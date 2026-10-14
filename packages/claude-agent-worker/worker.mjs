import { once } from "node:events";

import { query } from "@anthropic-ai/claude-agent-sdk";

import { diagnosticRedactor } from "./diagnostics.mjs";
import { AgentSession } from "./session.mjs";
import { requestLines } from "./transport.mjs";

if (Number(process.versions.node.split(".")[0]) < 22) {
  throw new Error("Claude Agent SDK requires Node.js 22 or later");
}

const emit = async (value) => {
  if (!process.stdout.write(`${JSON.stringify(value)}\n`)) {
    await once(process.stdout, "drain");
  }
};
const session = new AgentSession(query, emit);
const redact = diagnosticRedactor();
async function handleRequest(request) {
  try {
    const result = await session.call(request.method, request.params ?? {});
    if (request.id !== undefined) {
      await emit({ jsonrpc: "2.0", id: request.id, result });
    }
  } catch (error) {
    await emit(
      request.id === undefined
        ? { _runtime_error: redact(error.message ?? error) }
        : {
            jsonrpc: "2.0",
            id: request.id,
            error: { code: -32_000, message: redact(error.message ?? error) },
          },
    );
  }
}
async function run() {
  for await (const line of requestLines(process.stdin)) {
    let request;
    try {
      request = JSON.parse(line);
    } catch {
      await emit({ _runtime_error: "Invalid Claude SDK worker JSON" });
      session.close();
      return;
    }
    if (request.jsonrpc !== "2.0" || typeof request.method !== "string") {
      await emit({
        jsonrpc: "2.0",
        id: request.id ?? null,
        error: { code: -32_600, message: "Invalid SDK worker request" },
      });
      continue;
    }
    handleRequest(request).catch(() => session.close());
  }
}
async function reportFailure(error) {
  if (!session.closed) {
    await emit({ _runtime_error: redact(error.message ?? error) });
  }
}
void run()
  .catch(reportFailure)
  .finally(() => session.close());
process.on("SIGTERM", () => {
  session.close();
  process.stdin.destroy();
});
process.on("SIGINT", () => {
  session.close();
  process.stdin.destroy();
});
