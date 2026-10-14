import { Type } from "@earendil-works/pi-ai";
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
function checkedRecord(value: unknown): Record<string, unknown> {
  if (!isRecord(value)) {
    throw new TypeError("Invalid tool response.");
  }
  return value;
}
function checkedTool(value: unknown) {
  const tool = checkedRecord(value);
  if (
    typeof tool.command !== "string" ||
    typeof tool.name !== "string" ||
    typeof tool.description !== "string"
  ) {
    throw new TypeError("Invalid tool catalog entry.");
  }
  return {
    command: tool.command,
    name: tool.name,
    description: tool.description,
    inputSchema: checkedRecord(tool.inputSchema),
  };
}

/** Explicit native Pi tool bridge. Configure SWARMIE_RPC_URL and optional SWARMIE_TOKEN. */
export default function registerPeerTools(pi: ExtensionAPI) {
  const endpoint = process.env.SWARMIE_RPC_URL;
  if (endpoint === undefined || endpoint.length === 0) {
    return;
  }
  const token = process.env.SWARMIE_TOKEN;
  const allowed = new Set([
    "peers.spawn",
    "peers.read",
    "peers.send",
    "fanout.start",
    "fanout.status",
    "fanout.cancel",
  ]);
  let nextId = 1;
  async function rpc(command: string, args: unknown, signal?: AbortSignal) {
    const response = await fetch(endpoint, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        ...(token !== undefined && token.length > 0 ? { Authorization: `Bearer ${token}` } : {}),
      },
      body: JSON.stringify({
        jsonrpc: "2.0",
        id: (nextId += 1),
        method: "extension/invoke",
        params: { extensionId: "resources", command, arguments: args },
      }),
      signal: signal
        ? AbortSignal.any([signal, AbortSignal.timeout(30_000)])
        : AbortSignal.timeout(30_000),
    });
    if (!response.ok) {
      throw new Error(`swarmie HTTP ${response.status}`);
    }
    const value: unknown = await response.json();
    const body = checkedRecord(value);
    if (body.error !== undefined) {
      const failure = checkedRecord(body.error);
      throw new Error(
        typeof failure.message === "string"
          ? `swarmie: ${failure.message}`
          : "Swarmie rejected the tool request.",
      );
    }
    if (!Object.hasOwn(body, "result")) {
      throw new Error("Missing tool response result.");
    }
    return body.result;
  }
  pi.on("session_start", async () => {
    const catalog = checkedRecord(await rpc("tools.list", {}));
    if (!Array.isArray(catalog.tools)) {
      throw new TypeError("Invalid tool catalog.");
    }
    for (const value of catalog.tools) {
      const tool = checkedTool(value);
      if (!allowed.has(tool.command) || !/^[a-z_]+$/u.test(tool.name)) {
        continue;
      }
      pi.registerTool({
        name: tool.name,
        label: tool.name,
        description: tool.description,
        parameters: Type.Unsafe<unknown>(tool.inputSchema),
        async execute(_id, params, signal) {
          const details = await rpc(tool.command, params, signal);
          const full = JSON.stringify(details) ?? "null";
          const text =
            full.length > 24_000
              ? `${full.slice(0, 24_000)}\n[Output truncated; use peers_read with a sequence cursor]`
              : full;
          return { content: [{ type: "text", text }], details };
        },
      });
    }
  });
}
