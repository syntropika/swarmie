import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { Type } from "@earendil-works/pi-ai";

/** Explicit native Pi tool bridge. Configure SWARMIE_RPC_URL and optional SWARMIE_TOKEN. */
export default function (pi: ExtensionAPI) {
  const endpoint = process.env.SWARMIE_RPC_URL;
  if (!endpoint) return;
  const allowed = new Set(["peers.spawn", "peers.read", "peers.send", "fanout.start", "fanout.status", "fanout.cancel"]);
  let nextId = 1;
  async function rpc(command: string, args: unknown, signal?: AbortSignal) {
    const response = await fetch(endpoint!, {
      method: "POST",
      headers: { "Content-Type": "application/json", ...(process.env.SWARMIE_TOKEN ? { Authorization: `Bearer ${process.env.SWARMIE_TOKEN}` } : {}) },
      body: JSON.stringify({ jsonrpc: "2.0", id: nextId++, method: "extension/invoke", params: { extensionId: "resources", command, arguments: args } }),
      signal: signal ? AbortSignal.any([signal, AbortSignal.timeout(30_000)]) : AbortSignal.timeout(30_000),
    });
    if (!response.ok) throw new Error(`swarmie HTTP ${response.status}`);
    const body = await response.json();
    if (body.error) throw new Error(`swarmie: ${body.error.message}`);
    return body.result;
  }
  pi.on("session_start", async () => {
    const catalog = await rpc("tools.list", {});
    for (const tool of catalog.tools ?? []) {
      if (!allowed.has(tool.command) || !/^[a-z_]+$/.test(tool.name)) continue;
      pi.registerTool({
        name: tool.name,
        label: tool.name,
        description: tool.description,
        parameters: Type.Unsafe(tool.inputSchema),
        async execute(_id, params, signal) {
          const details = await rpc(tool.command, params, signal);
          const full = JSON.stringify(details);
          const text = full.length > 24_000 ? `${full.slice(0, 24_000)}\n[Output truncated; use peers_read with a sequence cursor]` : full;
          return { content: [{ type: "text", text }], details };
        },
      });
    }
  });
}
