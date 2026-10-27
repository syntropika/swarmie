import type {
  McpDefinition,
  McpEndpoint,
  McpProtocol,
  McpServer,
  McpConnectionStatus,
} from "./types";

export function object(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
function protocol(value: unknown): value is McpProtocol {
  return value === "2026-07-28" || value === "2025-11-25";
}
function strings(value: unknown): value is string[] {
  return Array.isArray(value) && value.every((entry: unknown) => typeof entry === "string");
}
function stringMap(value: unknown): value is Record<string, string> {
  return object(value) && Object.values(value).every((entry: unknown) => typeof entry === "string");
}
function endpoint(value: unknown): value is McpEndpoint {
  if (!object(value)) {
    return false;
  }
  return value.kind === "http"
    ? typeof value.url === "string" && stringMap(value.headers)
    : value.kind === "local" &&
        typeof value.program === "string" &&
        strings(value.arguments) &&
        (value.directory === null || typeof value.directory === "string");
}
export function definition(value: unknown): value is McpDefinition {
  return (
    object(value) &&
    typeof value.id === "string" &&
    typeof value.name === "string" &&
    endpoint(value.endpoint) &&
    protocol(value.protocol) &&
    typeof value.enabled === "boolean" &&
    typeof value.revision === "number" &&
    Number.isSafeInteger(value.revision) &&
    value.revision > 0 &&
    object(value.scope) &&
    value.scope.kind === "user"
  );
}
function status(value: unknown): value is McpConnectionStatus {
  return (
    value === "disconnected" ||
    value === "connecting" ||
    value === "connected" ||
    value === "disabled" ||
    value === "unavailable" ||
    value === "closed"
  );
}
export function servers(value: unknown): McpServer[] {
  if (!Array.isArray(value)) {
    throw new TypeError("The node returned an invalid MCP server list.");
  }
  return value.map((entry: unknown) => {
    if (
      !object(entry) ||
      !definition(entry.definition) ||
      !object(entry.observation) ||
      !status(entry.observation.status) ||
      !(entry.observation.protocol === null || protocol(entry.observation.protocol)) ||
      !Array.isArray(entry.observation.tools)
    ) {
      throw new Error("The node returned invalid MCP server details.");
    }
    return {
      definition: entry.definition,
      observation: {
        status: entry.observation.status,
        protocol: entry.observation.protocol,
        tools: entry.observation.tools,
      },
    };
  });
}

/** Match the native endpoint rules before saving a public server definition. */
export function validateDefinition(value: McpDefinition): void {
  if (
    !/^[A-Za-z0-9_-]{1,64}$/u.test(value.id) ||
    value.name.trim() === "" ||
    value.name.length > 128
  ) {
    throw new Error("Enter a server name of 1 to 128 characters.");
  }
  if (value.endpoint.kind === "local") {
    if (value.endpoint.program.trim() === "") {
      throw new Error("Enter the installed server command.");
    }
    return;
  }
  const url = new URL(value.endpoint.url);
  if (!["http:", "https:"].includes(url.protocol) || url.username || url.password || url.hash) {
    throw new Error("Use an HTTP or HTTPS URL without credentials or a fragment.");
  }
  const seen = new Set<string>();
  if (Object.keys(value.endpoint.headers).length > 32) {
    throw new Error("Use at most 32 custom headers.");
  }
  for (const [key, content] of Object.entries(value.endpoint.headers)) {
    const name = key.toLowerCase();
    if (
      !/^[!#$%&'*+.^_`|~0-9a-z-]+$/u.test(name) ||
      /[\r\n]/u.test(content) ||
      content.length > 4096
    ) {
      throw new Error("Enter a valid header name and a value without line breaks.");
    }
    if (seen.has(name)) {
      throw new Error("Each header name must be unique.");
    }
    seen.add(name);
    if (
      [
        "authorization",
        "proxy-authorization",
        "cookie",
        "x-api-key",
        "host",
        "content-length",
        "connection",
        "transfer-encoding",
        "mcp-session-id",
        "mcp-protocol-version",
        "accept",
        "content-type",
      ].includes(name)
    ) {
      throw new Error("Authentication and transport headers cannot be saved as custom headers.");
    }
  }
}
