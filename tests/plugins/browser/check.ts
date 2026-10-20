import { mkdtemp, readFile, rm } from "node:fs/promises";
import os from "node:os";
import path from "node:path";

import { bundleModule } from "./build";

const root = path.resolve(import.meta.dirname, "../../..");
const temporary = await mkdtemp(path.join(os.tmpdir(), "swarmie-browser-"));
const session = `swarmie-plugins-${process.pid}`;
const executable = process.env.AGENT_BROWSER ?? "agent-browser";
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
    path.join(root, "target/fixtures"),
  ],
  { cwd: root, stdin: "pipe", stdout: "pipe", stderr: "inherit" },
);

type Announcement = {
  readonly address: string;
  readonly session_id: string;
};
async function* announcements() {
  const reader = supervisor.stdout.getReader();
  let pending = "";
  const decoder = new TextDecoder();
  try {
    for (;;) {
      // oxlint-disable-next-line no-await-in-loop -- Preserve the order of the supervisor stdout stream.
      const chunk = await reader.read();
      if (chunk.done) {
        break;
      }
      pending += decoder.decode(chunk.value, { stream: true });
      let newline = pending.indexOf("\n");
      while (newline >= 0) {
        const line = pending.slice(0, newline);
        pending = pending.slice(newline + 1);
        const message: unknown = JSON.parse(line);
        if (
          message !== null &&
          typeof message === "object" &&
          "address" in message &&
          typeof message.address === "string" &&
          "session_id" in message &&
          typeof message.session_id === "string"
        ) {
          yield { address: message.address, session_id: message.session_id };
        }
        newline = pending.indexOf("\n");
      }
    }
  } finally {
    reader.releaseLock();
  }
}
const messages = announcements();
async function nextAnnouncement(): Promise<Announcement> {
  const next = await messages.next();
  if (next.done === true) {
    throw new Error("Browser supervisor exited before its lifecycle announcement");
  }
  return next.value;
}
async function browser(args: string[], input?: string) {
  const process = Bun.spawn([executable, "--session", session, ...args], {
    cwd: root,
    stdin: input === undefined ? "ignore" : new Blob([input]),
    stdout: "pipe",
    stderr: "inherit",
  });
  const output = await new Response(process.stdout).text();
  if ((await process.exited) !== 0) {
    throw new Error(`Browser command failed: ${args[0]}`);
  }
  return output.trim();
}
async function control(command: string) {
  await supervisor.stdin.write(`${command}\n`);
  await supervisor.stdin.flush();
  return await nextAnnouncement();
}
/** The trusted supervisor owns reporting; remote browser APIs expose code delivery only. */
async function reportObservation(observation: unknown): Promise<void> {
  if (
    observation === null ||
    typeof observation !== "object" ||
    !("plugin_id" in observation) ||
    typeof observation.plugin_id !== "string" ||
    !("version" in observation) ||
    typeof observation.version !== "string" ||
    !("activation_id" in observation) ||
    typeof observation.activation_id !== "string" ||
    !("observed" in observation) ||
    (observation.observed !== "active" && observation.observed !== "inactive")
  ) {
    throw new TypeError("Malformed observed browser execution");
  }
  await supervisor.stdin.write(
    `${JSON.stringify({ action: "report", plugin_id: observation.plugin_id, version: observation.version, activation_id: observation.activation_id, observed: observation.observed, error: "" })}\n`,
  );
  await supervisor.stdin.flush();
  await nextAnnouncement();
}

async function reportObservations(result: unknown): Promise<void> {
  if (result === null || typeof result !== "object") {
    throw new TypeError("Invalid browser observation result");
  }
  let observations: readonly unknown[] = [];
  if ("receipts" in result && Array.isArray(result.receipts)) {
    observations = result.receipts;
  } else if ("receipt" in result) {
    observations = [result.receipt];
  }
  if (observations.length === 0) {
    throw new Error("Browser execution produced no observation");
  }
  for (const observation of observations) {
    // oxlint-disable-next-line no-await-in-loop -- The supervisor must record old retirement before the replacement's activation.
    await reportObservation(observation);
  }
}
let server: ReturnType<typeof Bun.serve> | undefined;
let cleanupFailure: unknown;
let supervisorStatus: number | undefined;
try {
  const bootstrap = await nextAnnouncement();
  const entries = {
    "/sdk.js": "tests/plugins/browser/sdk.ts",
    "/contracts.js": "examples/plugins/browser-widget/contracts.ts",
    "/acceptance.js": "tests/plugins/browser/acceptance.ts",
    "/react.js": "tests/plugins/browser/react.ts",
    "/failing.js": "tests/plugins/browser/fixtures/failing.ts",
    "/incompatible.js": "tests/plugins/browser/fixtures/incompatible.ts",
  };
  const assets = new Map<string, string>(
    await Promise.all(
      Object.entries(entries).map(
        async ([url, entry]) =>
          [
            url,
            await bundleModule(entry, {
              sharedContracts: url === "/acceptance.js",
              includeDependencies: url === "/sdk.js" || url === "/react.js",
            }),
          ] as const,
      ),
    ),
  );
  const imports = {
    "@tembra/sdk": "/sdk.js",
    "@tembra/sdk/browser": "/sdk.js",
    react: "/react.js",
    "react-dom": "/react.js",
    "react-dom/client": "/react.js",
    "swarmie.acceptance.contracts": "/contracts.js",
  };
  const html = `<!doctype html><html lang="en"><head><meta charset="utf-8"><title>Plugin Browser Host acceptance</title><script type="importmap">${JSON.stringify({ imports })}</script></head><body><main><h1>Plugin Browser Host acceptance</h1><div id="contribution"></div><output id="result">running</output></main><script>addEventListener("error",event=>{document.querySelector("#result").textContent=JSON.stringify({failed:event.message});});addEventListener("unhandledrejection",event=>{document.querySelector("#result").textContent=JSON.stringify({failed:String(event.reason)});});</script><script type="module" src="/acceptance.js"></script></body></html>`;
  server = Bun.serve({
    hostname: "127.0.0.1",
    port: 0,
    async fetch(request) {
      const url = new URL(request.url);
      if (url.pathname === "/") {
        return new Response(html, { headers: { "content-type": "text/html" } });
      }
      if (url.pathname === "/bootstrap") {
        return Response.json(bootstrap, { headers: { "cache-control": "no-store" } });
      }
      const asset = assets.get(url.pathname);
      if (asset !== undefined) {
        return new Response(asset, { headers: { "content-type": "text/javascript" } });
      }
      if (
        url.pathname === "/wasm/tembra_browser_wasm.js" ||
        url.pathname === "/wasm/tembra_browser_wasm_bg.wasm" ||
        /^\/wasm\/snippets\/[a-zA-Z0-9_-]+\/inline[0-9]+\.js$/u.test(url.pathname)
      ) {
        const file = path.join(root, "target/browser-wasm", url.pathname.slice("/wasm/".length));
        return new Response(await readFile(file), {
          headers: {
            "content-type": file.endsWith(".wasm") ? "application/wasm" : "text/javascript",
          },
        });
      }
      return new Response("Not found", { status: 404 });
    },
  });
  await browser(["open", server.url.href]);
  try {
    await browser(["wait", "--fn", "document.querySelector('#result').textContent !== 'running'"]);
  } catch (error) {
    console.error("Page markup", await browser(["get", "html", "html"]));
    console.error("Browser stage", await browser(["get", "attr", "#result", "data-stage"]));
    console.error(await browser(["errors"]));
    console.error(await browser(["console"]));
    throw error;
  }
  const installed: unknown = JSON.parse(await browser(["get", "text", "#result"]));
  if (installed === null || typeof installed !== "object" || !("installed" in installed)) {
    throw new Error(JSON.stringify(installed));
  }
  await reportObservations(installed);
  await control("update");
  const updated = await browser(
    ["eval", "--stdin"],
    "(async () => await window.acceptance.update())()",
  );
  const updateResult: unknown = JSON.parse(updated);
  await reportObservations(updateResult);
  await control("deactivate");
  const retired = await browser(
    ["eval", "--stdin"],
    "(async () => await window.acceptance.retire())()",
  );
  const retirementResult: unknown = JSON.parse(retired);
  await reportObservations(retirementResult);
  console.log(JSON.stringify({ installed, updated: updateResult, retired: retirementResult }));
} finally {
  try {
    await browser(["close"]);
  } catch (error) {
    cleanupFailure = error;
  }
  await server?.stop(true);
  await supervisor.stdin.write("close\n");
  await supervisor.stdin.flush();
  await supervisor.stdin.end();
  const timeout = setTimeout(() => {
    supervisor.kill();
  }, 10_000);
  supervisorStatus = await supervisor.exited;
  clearTimeout(timeout);
  await rm(temporary, { recursive: true, force: true });
}
if (cleanupFailure !== undefined) {
  throw new Error("Browser cleanup failed", { cause: cleanupFailure });
}
if (supervisorStatus !== 0) {
  throw new Error(`Browser supervisor cleanup failed with status ${supervisorStatus}`);
}
