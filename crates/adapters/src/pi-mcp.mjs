import { readFileSync, writeFileSync, renameSync } from "node:fs";

export default function registerMcpServers(pi) {
  if (typeof pi.registerMcpServer !== "function") {
    throw new TypeError("This Pi version does not support session MCP registration.");
  }
  const manifest = JSON.parse(readFileSync(process.env.SWARMIE_PI_MCP_MANIFEST, "utf-8"));
  for (const server of manifest.servers) {
    pi.registerMcpServer(server.name, server.config);
  }

  const statusPath = process.env.SWARMIE_PI_MCP_STATUS;
  let timer;
  const publish = (observe = false) => {
    const tools = observe ? pi.getAllTools() : [];
    const rows = manifest.names.map((server) => {
      const prefix = `mcp__${server.name.replaceAll("-", "_")}__`;
      const names = tools
        .filter((tool) => tool.name.startsWith(prefix))
        .slice(0, 100)
        .map((tool) => tool.name.slice(prefix.length, prefix.length + 256));
      return {
        name: server.name,
        runtimeStatus: runtimeStatus(server.enabled, names),
        authStatus: "unknown",
        tools: names,
        canAuthenticate: false,
      };
    });
    writeFileSync(`${statusPath}.tmp`, JSON.stringify(rows), { mode: 0o600 });
    renameSync(`${statusPath}.tmp`, statusPath);
  };
  publish();
  pi.on("session_start", () => {
    publish(true);
    timer = setInterval(() => publish(true), 500);
    timer.unref();
  });
  pi.on("session_shutdown", () => clearInterval(timer));

  pi.on("tool_call", async (event, ctx) => {
    if (!event.toolName.startsWith("mcp__")) {
      return undefined;
    }
    const allowed = await ctx.ui.confirm("MCP tool permission", `Allow ${event.toolName}?`, {
      signal: ctx.signal,
    });
    if (!allowed) {
      return { block: true, reason: "MCP tool permission denied" };
    }
    return undefined;
  });
}

function runtimeStatus(enabled, names) {
  if (!enabled) {
    return "disabled";
  }
  return names.length > 0 ? "tools-available" : "unknown";
}
