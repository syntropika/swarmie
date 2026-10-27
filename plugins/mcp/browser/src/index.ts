import { updatePresentation } from "@swarmie/client-ui";
import { orb } from "@syntropika/tembra-sdk";

import type { McpState } from "./types";

export type {
  McpDefinition,
  McpEndpoint,
  McpProtocol,
  McpScope,
  McpServer,
  McpState,
} from "./types";
export { createMcpApi } from "./api";
export { validateDefinition } from "./validation";
export const mcpOrb = orb({
  name: "swarmie.mcp.ui",
  apiVersion: 1,
  services: {
    "swarmie.mcp.ui": {
      create: (self) =>
        self.state<McpState>({
          servers: [],
          loading: false,
          busy: null,
          error: null,
          available: false,
          nodeId: null,
          principal: null,
        }),
      methods: { update: updatePresentation<McpState> },
    },
  },
});
