export type McpEndpoint =
  | { kind: "http"; url: string; headers: Record<string, string> }
  | { kind: "local"; program: string; arguments: string[]; directory: string | null };
export type McpProtocol = "2026-07-28" | "2025-11-25";
export type McpScope = { kind: "user" };
export type McpDefinition = {
  id: string;
  name: string;
  scope: McpScope;
  endpoint: McpEndpoint;
  protocol: McpProtocol;
  enabled: boolean;
  revision: number;
};
export type McpConnectionStatus =
  | "disconnected"
  | "connecting"
  | "connected"
  | "disabled"
  | "unavailable"
  | "closed";
export type McpServer = {
  definition: McpDefinition;
  observation: { status: McpConnectionStatus; protocol: McpProtocol | null; tools: unknown[] };
};
export type McpState = {
  servers: McpServer[];
  loading: boolean;
  busy: string | null;
  error: string | null;
  available: boolean;
  nodeId: string | null;
  principal: string | null;
};
