import type { DynamicReference } from "@syntropika/tembra-sdk";

export const ConnectionPhase = {
  Unconfigured: "unconfigured",
  Disconnected: "disconnected",
  Connecting: "connecting",
  Connected: "connected",
  Reconnecting: "reconnecting",
  Failed: "failed",
} as const;
export type ConnectionPhaseValue = (typeof ConnectionPhase)[keyof typeof ConnectionPhase];

export const ConnectionFailureKind = {
  Network: "network",
  Configuration: "configuration",
  Identity: "identity",
  Principal: "principal",
  Version: "version",
  Protocol: "protocol",
  Resource: "resource",
  ScopeChanged: "scope-changed",
} as const;
export type ConnectionFailureKindValue =
  (typeof ConnectionFailureKind)[keyof typeof ConnectionFailureKind];

/** Public identity expectations; credentials belong to the injected connector. */
export type Node = {
  id: string;
  label: string;
  url: string | null;
  principal: string;
};
export type NodeIdentity = {
  nodeId: string;
  principal: string;
  protocolVersion: number;
};
export const NodeProtocol = {
  Service: "swarmie.node",
  Key: "main",
  Version: 1,
  MaxFrame: 64 * 1024,
  MaxPending: 16,
} as const;
export type ConnectionFailure = { kind: ConnectionFailureKindValue; message: string };
export type NodeConnectionState = {
  nodeId: string;
  phase: ConnectionPhaseValue;
  generation: number;
  failure: ConnectionFailure | null;
};
export type ConnectionsState = {
  nodes: Node[];
  selected: string;
  sessions: Record<string, NodeConnectionState>;
};

/** A connector returns one verified channel and never owns reconnection policy. */
export type NodeSession = {
  identity: NodeIdentity;
  get: (
    service: string,
    key: string | readonly string[],
    options?: { signal?: AbortSignal },
  ) => Promise<DynamicReference>;
  close: () => void;
};
export type Connector = (
  node: Node,
  signal: AbortSignal,
  lost: (error: unknown) => void,
) => Promise<NodeSession>;

export const unconfiguredNode: Node = {
  id: "local",
  label: "Local",
  url: null,
  principal: "local",
};
