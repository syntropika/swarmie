import { createHash } from "node:crypto";
import { realpath } from "node:fs/promises";
import { cpus, release, totalmem } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

import type { PerformanceCase } from "./cases";
import { finite, record } from "./model";

export const repository = fileURLToPath(new URL("../../", import.meta.url));

async function resolveExisting(candidate: string): Promise<string> {
  try {
    return await realpath(candidate);
  } catch (error: unknown) {
    if (!(error instanceof Error) || !("code" in error) || error.code !== "ENOENT") {
      throw error;
    }
    const parent = path.dirname(candidate);
    if (parent === candidate) {
      throw new Error("No existing output ancestor.", { cause: error });
    }
    return path.join(await resolveExisting(parent), path.basename(candidate));
  }
}

export async function externalPath(candidate: string): Promise<string> {
  const absolute = path.resolve(candidate);
  const resolved = await resolveExisting(absolute);
  const root = await realpath(repository);
  const relative = path.relative(root, resolved);
  if (
    relative === "" ||
    (!relative.startsWith(`..${path.sep}`) && relative !== ".." && !path.isAbsolute(relative))
  ) {
    throw new Error("Performance output must remain outside the source repository.");
  }
  return resolved;
}

async function command(args: string[]): Promise<string> {
  const child = Bun.spawn(args, {
    cwd: repository,
    stdout: "pipe",
    stderr: "pipe",
    ...(args[0] === "git"
      ? {
          env: {
            ...Object.fromEntries(
              Object.entries(process.env).filter(([key]) => !key.startsWith("GIT_")),
            ),
            GIT_OPTIONAL_LOCKS: "0",
            GIT_TERMINAL_PROMPT: "0",
          },
        }
      : {}),
  });
  const [stdout, stderr, code] = await Promise.all([
    new Response(child.stdout).text(),
    new Response(child.stderr).text(),
    child.exited,
  ]);
  if (code !== 0) {
    throw new Error(`${args[0]} failed (${code}): ${stderr.slice(-8000)}`);
  }
  return stdout.trim();
}

async function nativeBinary(rust: string, agent = false): Promise<string> {
  const name = agent ? "swarmie-fixture-agent-performance" : "swarmie-fixture-performance";
  const output = await command([
    "cargo",
    `+${rust}`,
    "build",
    "--locked",
    "--release",
    "-p",
    "swarmie-fixture-performance",
    "--bin",
    name,
    ...(agent ? ["--features", "agent-turns"] : []),
    "--message-format=json",
  ]);
  for (const line of output.split("\n").toReversed()) {
    const message = record(JSON.parse(line));
    if (
      message.reason === "compiler-artifact" &&
      record(message.target).name === name &&
      typeof message.executable === "string"
    ) {
      return message.executable;
    }
  }
  throw new Error("Cargo did not report the performance worker executable.");
}

export async function sourceIdentity() {
  const hash = createHash("sha256").update(
    await command([
      "git",
      "diff",
      "--no-ext-diff",
      "--no-textconv",
      "--binary",
      "--full-index",
      "HEAD",
    ]),
  );
  const untracked = await command(["git", "ls-files", "--others", "--exclude-standard", "-z"]);
  const files = await Promise.all(
    untracked
      .split("\0")
      .filter(Boolean)
      .map(async (file) => ({ file, bytes: await Bun.file(path.join(repository, file)).bytes() })),
  );
  for (const { file, bytes } of files) {
    hash.update(`${file}\0${bytes.length}\0`).update(bytes);
  }
  const [revision, status] = await Promise.all([
    command(["git", "rev-parse", "HEAD"]),
    command(["git", "status", "--porcelain"]),
  ]);
  return { revision, dirty: status.length > 0, worktreeSha256: hash.digest("hex") };
}

export type Sample = {
  metrics: Record<string, number>;
  resources: Record<string, number>;
  verified: Record<string, unknown>;
};

/** Bun 1.4 returns CPU microseconds as bigint; the pinned ambient types use number. */
export function resourceNumber(value: unknown): number {
  if (typeof value === "bigint") {
    if (value < 0n || value > BigInt(Number.MAX_SAFE_INTEGER)) {
      throw new RangeError("Resource usage exceeds the exact numeric report range.");
    }
    return Number(value);
  }
  return finite(value);
}

export async function sample(
  entry: PerformanceCase,
  binary: string,
  fixtures: string,
  timeoutMs: number,
): Promise<Sample> {
  const args =
    entry.engine === "bun"
      ? [
          process.execPath,
          path.join(repository, "scripts/performance/render.tsx"),
          entry.operation,
          String(entry.size),
        ]
      : [binary, entry.operation, String(entry.size), fixtures, String(timeoutMs)];
  const child = Bun.spawn(args, {
    cwd: repository,
    env: { ...process.env, NODE_ENV: "production", NODE_OPTIONS: "", BUN_OPTIONS: "" },
    stdout: "pipe",
    stderr: "pipe",
    ...(entry.engine === "rust" ? {} : { timeout: timeoutMs }),
  });
  const [stdout, stderr, code] = await Promise.all([
    new Response(child.stdout).text(),
    new Response(child.stderr).text(),
    child.exited,
  ]);
  if (code !== 0 || child.signalCode !== null) {
    throw new Error(
      `${entry.id} failed (${code}, ${child.signalCode ?? "no signal"}): ${stderr.slice(-8000)}`,
    );
  }
  const payload = record(JSON.parse(stdout));
  const metrics = Object.fromEntries(
    Object.entries(payload)
      .filter(([key]) => key !== "verified_items")
      .map(([key, value]) => [key, finite(value)]),
  );
  if (metrics.elapsed_ms === undefined || metrics.elapsed_ms > timeoutMs) {
    throw new Error(`${entry.id} exceeded its measured deadline or omitted elapsed time.`);
  }
  const usage = child.resourceUsage();
  const resources =
    usage === undefined
      ? {}
      : {
          worker_peak_rss_bytes: resourceNumber(usage.maxRSS),
          worker_cpu_ms: resourceNumber(usage.cpuTime.total) / 1000,
        };
  return { metrics, resources, verified: record(payload.verified_items) };
}

export async function prepare(selected: readonly PerformanceCase[] = []) {
  const tools = await Bun.file(path.join(repository, "mise.toml")).text();
  const rust = /^rust = "(?<version>[\d.]+)"$/mu.exec(tools)?.groups?.version;
  const manager = record(
    await Bun.file(path.join(repository, "package.json")).json(),
  ).packageManager;
  if (rust === undefined || manager !== `bun@${Bun.version}`) {
    throw new Error("Use the root pinned Rust and Bun toolchain.");
  }
  const source = await sourceIdentity();
  console.log("Building the locked release worker; build time is outside workload timing.");
  const binary = await nativeBinary(rust);
  const agentBinary = selected.some((entry) => entry.engine === "rust-agent")
    ? await nativeBinary(rust, true)
    : null;
  const agentBinarySha256 =
    agentBinary === null
      ? undefined
      : createHash("sha256")
          .update(await Bun.file(agentBinary).bytes())
          .digest("hex");
  const nativeBinarySha256 = createHash("sha256")
    .update(await Bun.file(binary).bytes())
    .digest("hex");
  const [compiler, git] = await Promise.all([
    command(["rustc", `+${rust}`, "--version"]),
    command(["git", "--version"]),
  ]);
  const environment = {
    os: process.platform,
    arch: process.arch,
    release: release(),
    cpu: cpus()[0]?.model ?? "unknown",
    logical_cpus: cpus().length,
    memory_bytes: totalmem(),
    bun: Bun.version,
    rust: compiler,
    git,
    node_env: "production",
    build: "release",
    tuning_sha256: createHash("sha256")
      .update(
        JSON.stringify(
          Object.entries(process.env)
            .filter(([key]) =>
              /^(?:RUSTFLAGS$|CARGO_ENCODED_RUSTFLAGS$|CARGO_PROFILE_|CARGO_BUILD_TARGET$|BUN_JSC_|JSC_)/u.test(
                key,
              ),
            )
            .toSorted(([left], [right]) => left.localeCompare(right)),
        ),
      )
      .digest("hex"),
  };
  return {
    binary,
    agentBinary,
    source: {
      ...source,
      nativeBinarySha256,
      ...(agentBinarySha256 === undefined ? {} : { agentBinarySha256 }),
    },
    environment,
  };
}
