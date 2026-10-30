import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdir } from "node:fs/promises";
import { cpus, loadavg, release, totalmem } from "node:os";

import { finite, record, statistics } from "./model";
import { externalPath, repository, sourceIdentity } from "./workers";

export function clientSample(value: unknown, prior: number, repetitions: number) {
  const result = record(value);
  assert.equal(result.prior_messages, prior);
  assert.equal(result.message_bytes, 128);
  assert.equal(result.warmups, 5);
  assert.equal(result.model_calls, 1);
  assert.equal(result.completion_verified, true);
  assert.ok(Array.isArray(result.turn_samples_ns));
  assert.equal(result.turn_samples_ns.length, repetitions);
  return result.turn_samples_ns.map((sample: unknown) => finite(sample) / 1_000_000);
}

export function clientOutput(output: string): unknown {
  const marker = "PERF_CLIENT_TURN ";
  const lines = output.split("\n").filter((line) => line.includes(marker));
  assert.equal(lines.length, 1, "native probe must report one verified timing payload");
  const [line] = lines;
  assert.ok(line !== undefined);
  return JSON.parse(line.slice(line.indexOf(marker) + marker.length));
}

export async function runClient(options: {
  output: string;
  samples: number;
  warmups: number;
  timeoutMs: number;
  full: boolean;
}) {
  const output = await externalPath(options.output);
  assert.equal(
    record(await Bun.file(`${repository}/package.json`).json()).packageManager,
    `bun@${Bun.version}`,
    "Use the root pinned Bun toolchain.",
  );
  await mkdir(output);
  const source = await sourceIdentity();
  const toolchain = await Bun.file(`${repository}/mise.toml`).text();
  const rust = /^rust = "(?<version>[^"]+)"$/mu.exec(toolchain)?.groups?.version;
  assert.ok(rust !== undefined, "missing pinned Rust toolchain");
  const build = Bun.spawn(
    [
      "cargo",
      `+${rust}`,
      "test",
      "--locked",
      "--release",
      "-p",
      "swarmie-desktop-runtime",
      "--test",
      "conversations",
      "--no-run",
      "--message-format=json",
    ],
    { cwd: repository, stdout: "pipe", stderr: "pipe" },
  );
  const [stdout, stderr, buildCode] = await Promise.all([
    new Response(build.stdout).text(),
    new Response(build.stderr).text(),
    build.exited,
  ]);
  await Bun.write(`${output}/build.jsonl`, stdout);
  await Bun.write(`${output}/build.stderr`, stderr);
  assert.equal(buildCode, 0, stderr);
  let binary: string | undefined;
  for (const line of stdout.trim().split("\n")) {
    const message = record(JSON.parse(line));
    if (
      message.reason === "compiler-artifact" &&
      record(message.target).name === "conversations" &&
      typeof message.executable === "string"
    ) {
      binary = message.executable;
    }
  }
  assert.ok(binary !== undefined, "Cargo did not report the native client probe");
  const nativeBinary = binary;
  const digest = async () =>
    createHash("sha256")
      .update(await Bun.file(nativeBinary).bytes())
      .digest("hex");
  const binarySha256 = await digest();
  const repetitions = options.full ? 60 : 20;
  const environment = Object.fromEntries(
    Object.entries(process.env).filter(([key]) => ["PATH", "LANG", "LC_ALL", "TZ"].includes(key)),
  );
  const loadBefore = loadavg();
  const runs: { prior: number; phase: string; file: string; samples_ms: number[] }[] = [];
  let interrupted = false;
  const interrupt = () => {
    interrupted = true;
  };
  process.on("SIGINT", interrupt);
  process.on("SIGTERM", interrupt);
  const history = [0, 100, 1000];
  const jobs = Array.from({ length: options.warmups + options.samples }, (_, round) => {
    const offset = round % history.length;
    return [...history.slice(offset), ...history.slice(0, offset)].map((prior) => ({
      prior,
      round,
      phase: round < options.warmups ? "warmup" : "sample",
    }));
  }).flat();
  // Workers run in sequence so that the probe does not measure its own contention.
  async function measureAt(index: number): Promise<void> {
    assert.ok(!interrupted, "interrupted after the current worker settled");
    const job = jobs[index];
    if (job === undefined) {
      return;
    }
    const { prior, round, phase } = job;
    const child: Bun.Subprocess<"ignore", "pipe", "pipe"> = Bun.spawn(
      [nativeBinary, "--ignored", "--exact", "persisted_client_answer_performance", "--nocapture"],
      {
        cwd: repository,
        env: {
          ...environment,
          RUST_TEST_THREADS: "1",
          SWARMIE_PERF_PRIOR: String(prior),
          SWARMIE_PERF_REPETITIONS: String(repetitions),
        },
        stdout: "pipe",
        stderr: "pipe",
        timeout: options.timeoutMs,
      },
    );
    const [out, err, code] = await Promise.all([
      new Response(child.stdout).text(),
      new Response(child.stderr).text(),
      child.exited,
    ]);
    const file = `${phase}-${round}-h${prior}`;
    await Promise.all([
      Bun.write(`${output}/${file}.stdout`, out),
      Bun.write(`${output}/${file}.stderr`, err),
    ]);
    assert.equal(code, 0, `${file}: ${out}\n${err}`);
    const value = clientOutput(out);
    await Bun.write(`${output}/${file}.json`, JSON.stringify(value, null, 2));
    runs.push({ prior, phase, file, samples_ms: clientSample(value, prior, repetitions) });
    console.log(`${phase} ${round}: native client answer, history ${prior} complete`);
    await measureAt(index + 1);
  }
  try {
    await measureAt(0);
    assert.equal(await digest(), binarySha256);
    assert.deepEqual(await sourceIdentity(), source, "source changed during measurement");
    const results = history.map((prior) => {
      const workers = runs.filter((run) => run.phase === "sample" && run.prior === prior);
      const medians = workers.map((run) => statistics(run.samples_ms).median);
      const variation = statistics(medians).cv;
      return {
        prior,
        ...statistics(workers.flatMap((run) => run.samples_ms)),
        worker_medians_ms: medians,
        worker_median_cv: variation,
        status: variation > 0.2 ? "unstable" : "descriptive",
      };
    });
    await Bun.write(
      `${output}/report.json`,
      JSON.stringify(
        {
          schema: 1,
          source,
          binary_sha256: binarySha256,
          rust,
          bun: Bun.version,
          environment: {
            platform: process.platform,
            arch: process.arch,
            release: release(),
            cpu: cpus()[0]?.model ?? "unknown",
            logical_cpus: cpus().length,
            memory_bytes: totalmem(),
          },
          scope:
            "native client RPC admission through durable task release and projection; one loopback HTTP model answer; includes IPC and observation costs; excludes real model generation and UI rendering",
          repetitions,
          samples: options.samples,
          warmups: options.warmups,
          load_before: loadBefore,
          load_after: loadavg(),
          results,
          runs,
        },
        null,
        2,
      ),
    );
    return results;
  } finally {
    process.off("SIGINT", interrupt);
    process.off("SIGTERM", interrupt);
  }
}
