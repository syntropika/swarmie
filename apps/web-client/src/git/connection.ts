import { useSource } from "@swarmie/client-ui";
import type { NodeConnection } from "@swarmie/plugin-connections-browser";
import { useEffect, useState } from "react";

import { useClient } from "../client/context";

/** Resolves only the captured node identity, never the current global selection. */
export function useGitConnection(nodeId: string, principal: string) {
  const { connectionState, nodeConnections } = useClient();
  const nodes = useSource(connectionState);
  const configured = nodes.nodes.find((node) => node.id === nodeId);
  const session = nodes.sessions[nodeId];
  const key = `${nodeId}:${principal}:${session?.generation ?? "none"}:${session?.phase ?? "none"}`;
  const [resolved, setResolved] = useState<{
    key: string;
    connection: NodeConnection | null;
    error: string | null;
  }>({ key: "", connection: null, error: null });
  useEffect(() => {
    let active = true;
    function failed(error: unknown) {
      if (active) {
        setResolved({
          key,
          connection: null,
          error: error instanceof Error ? error.message : String(error),
        });
      }
    }
    async function resolve() {
      if (configured === undefined || configured.principal !== principal) {
        throw new Error(
          "This tab's node identity is no longer configured. Open a new view to choose another node.",
        );
      }
      const connection = await nodeConnections.open(nodeId);
      if (active && connection.isCurrent() && connection.principal === principal) {
        setResolved({ key, connection, error: null });
      }
    }
    resolve().catch(failed);
    return () => {
      active = false;
    };
  }, [nodeConnections, nodeId, principal, configured, key]);
  if (resolved.key !== key || (resolved.connection !== null && !resolved.connection.isCurrent())) {
    return { connection: null, error: null };
  }
  return resolved;
}
