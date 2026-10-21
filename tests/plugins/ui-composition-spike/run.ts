import { createHash } from "node:crypto";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";

import tailwindcss from "@tailwindcss/vite";
import { build as buildStyles } from "vite";

import type { RemoteBrowserAccessMainActivePackageResult } from "../../../target/generated/contracts/remote-browser-access";

const root = path.resolve(import.meta.dirname, "../../..");
const interactive = Bun.argv.includes("--serve");
const temporary = await mkdtemp(path.join(os.tmpdir(), "swarmie-ui-spike-"));
const releases = path.join(root, "target/ui-composition-spike/releases");
const session = `swarmie-ui-spike-${process.pid}`;
const source = "tests/plugins/ui-composition-spike/";
const sha256 = (bytes: string | Uint8Array) => createHash("sha256").update(bytes).digest("hex");

async function bundle(entry: string, shared = true, dependencies = false) {
  const result = await Bun.build({
    entrypoints: [path.join(root, entry)],
    format: "esm",
    target: "browser",
    external: dependencies
      ? []
      : [
          "@tembra/sdk",
          "@tembra/sdk/browser",
          "react",
          "react-dom",
          "react-dom/client",
          ...(shared ? ["@swarmie/ui-web"] : []),
        ],
    plugins: [
      {
        name: "spike-shared-contracts",
        setup(builder) {
          builder.onResolve({ filter: /^\.\/contracts$/u }, (args) =>
            args.importer.startsWith(path.join(root, source))
              ? { path: "swarmie.spike.contracts", external: true }
              : undefined,
          );
        },
      },
    ],
  });
  const output = result.outputs.find((value) => value.path.endsWith(".js"));
  if (!result.success || !output || result.outputs.length !== 1) {
    throw new Error(
      result.logs.map(String).join("\n") || "Module must be one closed JavaScript bundle",
    );
  }
  return await output.text();
}

await Promise.all(
  (
    [
      ["1.0.0", "v1.ts"],
      ["1.1.0", "v2.ts"],
    ] as const
  ).map(async ([version, entry]) => {
    const original = path.join(root, "target/fixtures/multi-host-widget", version);
    const originalManifest: unknown = JSON.parse(
      await readFile(path.join(original, "manifest.json"), "utf-8"),
    );
    if (
      originalManifest === null ||
      typeof originalManifest !== "object" ||
      !("compatibility" in originalManifest) ||
      originalManifest.compatibility === null ||
      typeof originalManifest.compatibility !== "object" ||
      !("tembra_revision" in originalManifest.compatibility) ||
      typeof originalManifest.compatibility.tembra_revision !== "string"
    ) {
      throw new Error("Missing generated package compatibility");
    }
    const [server, facet, views] = await Promise.all([
      readFile(path.join(original, "server.json")),
      bundle(source + entry),
      bundle(`${source}views.ts`),
    ]);
    const files = {
      "server.json": server,
      "facet.js": facet,
      "views.js": views,
    };
    const manifest: RemoteBrowserAccessMainActivePackageResult["manifest"] = {
      schema_version: 1,
      id: "multi-host-widget",
      version,
      compatibility: {
        swarmie: "^0.1.0",
        tembra_revision: originalManifest.compatibility.tembra_revision,
      },
      dependencies: [],
      facets: [
        {
          id: "dynamic-consumer",
          target: "server",
          format: "bundled",
          artifact_path: "server.json",
          sha256: sha256(server),
          selected_export: "default",
        },
        {
          id: "browser-widget",
          target: "browser",
          format: "native_esm",
          artifact_path: "facet.js",
          sha256: sha256(facet),
          selected_export: "default",
        },
      ],
      files: Object.entries(files).map(([file, bytes]) => ({ path: file, sha256: sha256(bytes) })),
      distribution: { provider: "local", description: "Disposable UI composition spike" },
      ui: [
        {
          id: "counter.screen",
          plugin_id: "multi-host-widget",
          kind: "screen",
          label: "Counter screen",
          slot_id: "workspace.screen",
          order: 0,
          module_path: "views.js",
          export_name: "CounterScreen",
        },
      ],
    };
    const destination = path.join(releases, "multi-host-widget", version);
    await mkdir(destination, { recursive: true });
    await Promise.all([
      ...Object.entries(files).map(async ([file, bytes]) => {
        await writeFile(path.join(destination, file), bytes);
      }),
      writeFile(path.join(destination, "manifest.json"), JSON.stringify(manifest)),
    ]);
  }),
);
const assets = new Map<string, { bytes: string | Uint8Array; type: string }>();
await Promise.all(
  (
    [
      ["/sdk.js", "tests/plugins/browser/sdk.ts", true, true],
      ["/react.js", `${source}react.ts`, true, true],
      ["/contracts.js", `${source}contracts.ts`, true, false],
      ["/shell.js", `${source}shell.ts`, true, false],
      ["/client.js", `${source}client.ts`, true, false],
    ] as const
  ).map(async ([url, entry, shared, dependencies]) => {
    assets.set(url, { bytes: await bundle(entry, shared, dependencies), type: "text/javascript" });
  }),
);
const css = await buildStyles({
  configFile: false,
  root,
  plugins: [tailwindcss()],
  logLevel: "error",
  define: { "process.env.NODE_ENV": JSON.stringify("development") },
  build: {
    write: false,
    minify: false,
    cssMinify: false,
    cssCodeSplit: true,
    lib: { entry: path.join(root, source, "ui.ts"), formats: ["es"], fileName: () => "ui.js" },
    rolldownOptions: {
      input: {
        ui: path.join(root, source, "ui.ts"),
        styles: path.join(root, source, "styles.css"),
      },
      external: [
        "react",
        "react/jsx-runtime",
        "react/jsx-dev-runtime",
        "react-dom",
        "react-dom/client",
      ],
    },
  },
});
const builds = Array.isArray(css) ? css : [css];
const [outputBuild] = builds;
if (builds.length !== 1 || !outputBuild || !("output" in outputBuild)) {
  throw new Error("Expected one stylesheet build");
}
let stylesheet = "";
for (const output of outputBuild.output) {
  let type = "font/ttf";
  if (output.type === "chunk") {
    type = "text/javascript";
  } else if (output.fileName.endsWith(".css")) {
    type = "text/css";
  }
  assets.set(`/${output.fileName}`, {
    bytes: output.type === "asset" ? output.source : output.code,
    type,
  });
  if (output.fileName.endsWith(".css")) {
    stylesheet = `/${output.fileName}`;
  }
}
if (!stylesheet) {
  throw new Error("Missing shared stylesheet");
}

const supervisor = Bun.spawn(
  [
    "cargo",
    "+1.99.0",
    "run",
    "--locked",
    "--quiet",
    "-p",
    "swarmie-runtime",
    "--example",
    "browser_acceptance",
    "--",
    temporary,
    releases,
  ],
  {
    cwd: root,
    stdin: "pipe",
    stdout: "pipe",
    stderr: "inherit",
  },
);
const reader = supervisor.stdout.getReader();
const decoder = new TextDecoder();
let buffered = "";
async function announcement(): Promise<{ address: string; session_id: string }> {
  while (!buffered.includes("\n")) {
    // oxlint-disable-next-line no-await-in-loop -- Each stdout chunk completes the preceding partial announcement.
    const part = await reader.read();
    if (part.done) {
      throw new Error("Native supervisor exited before readiness");
    }
    buffered += decoder.decode(part.value, { stream: true });
  }
  const end = buffered.indexOf("\n");
  const parsed: unknown = JSON.parse(buffered.slice(0, end));
  buffered = buffered.slice(end + 1);
  if (
    parsed !== null &&
    typeof parsed === "object" &&
    "ready" in parsed &&
    parsed.ready === true &&
    "address" in parsed &&
    typeof parsed.address === "string"
  ) {
    // The runtime listener announces first; the supervisor then verifies the plugin.
    return await announcement();
  }
  if (
    parsed === null ||
    typeof parsed !== "object" ||
    !("address" in parsed) ||
    typeof parsed.address !== "string" ||
    !("session_id" in parsed) ||
    typeof parsed.session_id !== "string"
  ) {
    throw new Error(`Invalid native supervisor announcement: ${JSON.stringify(parsed)}`);
  }
  return { address: parsed.address, session_id: parsed.session_id };
}
const bootstrap = await announcement();
let queue = Promise.resolve();
async function command(value: string) {
  let response: unknown;
  queue = queue.then(async () => {
    await supervisor.stdin.write(`${value}\n`);
    await supervisor.stdin.flush();
    response = await announcement();
  });
  await queue;
  return response;
}
const imports = {
  "@tembra/sdk": "/sdk.js",
  "@tembra/sdk/browser": "/sdk.js",
  react: "/react.js",
  "react/jsx-runtime": "/react.js",
  "react/jsx-dev-runtime": "/react.js",
  "react-dom": "/react.js",
  "react-dom/client": "/react.js",
  "@swarmie/ui-web": "/ui.js",
  "swarmie.spike.contracts": "/contracts.js",
};
const html = `<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>UI composition spike</title><link rel="stylesheet" href="${stylesheet}"><script type="importmap">${JSON.stringify({ imports })}</script></head><body><div id="app"><p>Connecting to the runtime…</p></div><script type="module">import("/client.js").catch(error => { document.documentElement.dataset.failure = String(error); document.querySelector("#app").textContent = "Initialization failed: " + String(error); console.error(error); });</script></body></html>`;
const server = Bun.serve({
  hostname: "127.0.0.1",
  port: Number(process.env.SWARMIE_SPIKE_PORT ?? 0),
  async fetch(request) {
    const url = new URL(request.url);
    if (url.pathname === "/") {
      return new Response(html, { headers: { "content-type": "text/html" } });
    }
    if (url.pathname === "/bootstrap") {
      return Response.json(bootstrap);
    }
    if (url.pathname === "/control" && request.method === "POST") {
      // This loopback fixture controller is not a production remote administration API.
      if (
        request.headers.get("origin") !== url.origin ||
        request.headers.get("content-type") !== "application/json"
      ) {
        return new Response("Same-origin JSON required", { status: 403 });
      }
      const body: unknown = await request.json();
      if (body === null || typeof body !== "object" || !("command" in body)) {
        return new Response("Invalid command", { status: 400 });
      }
      if (body.command === "update" || body.command === "deactivate") {
        return Response.json(await command(body.command));
      }
      if (
        body.command === "report" &&
        "receipt" in body &&
        body.receipt !== null &&
        typeof body.receipt === "object"
      ) {
        return Response.json(await command(JSON.stringify({ ...body.receipt, action: "report" })));
      }
      return new Response("Unknown command", { status: 400 });
    }
    const asset = assets.get(url.pathname);
    if (asset) {
      return new Response(
        typeof asset.bytes === "string" ? asset.bytes : new Uint8Array(asset.bytes),
        { headers: { "content-type": asset.type } },
      );
    }
    if (
      /^\/wasm\/(?:tembra_browser_wasm\.(?:js)|tembra_browser_wasm_bg\.wasm|snippets\/[a-zA-Z0-9_-]+\/inline[0-9]+\.js)$/u.test(
        url.pathname,
      )
    ) {
      return new Response(
        await readFile(path.join(root, "target/browser-wasm", url.pathname.slice(6))),
        {
          headers: {
            "content-type": url.pathname.endsWith(".wasm") ? "application/wasm" : "text/javascript",
          },
        },
      );
    }
    return new Response("Not found", { status: 404 });
  },
});

async function browser(args: string[], input?: string) {
  const child = Bun.spawn(
    [process.env.AGENT_BROWSER ?? "agent-browser", "--session", session, ...args],
    {
      cwd: root,
      stdin: input === undefined ? "ignore" : new Blob([input]),
      stdout: "pipe",
      stderr: "inherit",
    },
  );
  const output = await new Response(child.stdout).text();
  if ((await child.exited) !== 0) {
    throw new Error(`Browser command failed: ${args[0]}`);
  }
  return output.trim();
}

async function close() {
  await server.stop(true);
  await supervisor.stdin.write("close\n");
  await supervisor.stdin.flush();
  await supervisor.stdin.end();
  const watchdog = setTimeout(() => {
    supervisor.kill();
  }, 10_000);
  const exit = await supervisor.exited;
  clearTimeout(watchdog);
  await rm(temporary, { recursive: true, force: true });
  if (exit !== 0) {
    throw new Error(`Native supervisor exited ${exit}`);
  }
}

if (interactive) {
  console.log(`UI composition spike: ${server.url}`);
  console.log(
    "Synthetic loopback fixture. Stop with Ctrl+C; no production authentication or durable UI history.",
  );
  process.on("SIGINT", () => {
    close()
      .catch(console.error)
      .finally(() => process.exit(0));
  });
  process.on("SIGTERM", () => {
    close()
      .catch(console.error)
      .finally(() => process.exit(0));
  });
} else {
  try {
    await browser(["open", server.url.href]);
    await browser([
      "wait",
      "--fn",
      "document.documentElement.dataset.ready === 'true' || Boolean(document.documentElement.dataset.failure)",
    ]);
    await browser([
      "eval",
      "if (document.documentElement.dataset.failure) throw new Error(document.documentElement.dataset.failure)",
    ]);
    const result = await browser(
      ["eval", "--stdin"],
      await readFile(path.join(root, source, "scenario.js"), "utf-8"),
    );
    console.log(result);
  } catch (error) {
    console.error(
      "Initialization stage:",
      await browser(["eval", "document.documentElement.dataset.stage ?? 'module-loading'"]),
    );
    console.error(await browser(["errors"]));
    console.error(await browser(["console"]));
    throw error;
  } finally {
    try {
      await browser(["close"]);
    } finally {
      await close();
    }
  }
}
