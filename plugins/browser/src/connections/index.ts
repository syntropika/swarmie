import { updatePresentation } from "@swarmie/client-ui";
import { orb } from "@syntropika/tembra-sdk";

import { unconfiguredNode } from "./types";
import type { ConnectionsState } from "./types";

export { Connections } from "./connections";
export { NodeConnection } from "./node-connection";
export { NodeConnectionError, connectionFailure } from "./errors";
export { ConnectionFailureKind, ConnectionPhase, NodeProtocol, unconfiguredNode } from "./types";
export type {
  ConnectionFailure,
  ConnectionFailureKindValue,
  ConnectionPhaseValue,
  ConnectionsState,
  Connector,
  Node,
  NodeConnectionState,
  NodeIdentity,
  NodeSession,
} from "./types";
export { connectWebSocket } from "./websocket";

export const connectionsOrb = orb({
  name: "swarmie.connections.ui",
  apiVersion: 1,
  services: {
    "swarmie.connections.ui": {
      create: (self) =>
        self.state<ConnectionsState>({
          nodes: [unconfiguredNode],
          selected: unconfiguredNode.id,
          sessions: {},
        }),
      methods: { update: updatePresentation<ConnectionsState> },
    },
  },
});
