import type { Connector, Node } from "@swarmie/plugin-connections-browser";
import { NodeProtocol } from "@swarmie/plugin-connections-browser";

export const fixtureNode: Node = {
  id: "fixture-node",
  label: "Fixture",
  url: null,
  principal: "fixture-user",
};
export const fixtureConnector: Connector = async (node) => {
  await Promise.resolve();
  return {
    identity: { nodeId: node.id, principal: node.principal, protocolVersion: NodeProtocol.Version },
    get: async () => {
      await Promise.resolve();
      throw new Error("This admission fixture does not acquire remote services.");
    },
    close: () => undefined,
  };
};
