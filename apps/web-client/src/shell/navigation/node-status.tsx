import { useSource } from "@swarmie/client-ui";
import { ChromeButton } from "@swarmie/ui-web";
import { Monitor } from "lucide-react";

import { useClient } from "../../client/context";
import { NodeMenu } from "../../node-menu";

export function NodeStatus() {
  const { connectionState, selectNode } = useClient();
  const connection = useSource(connectionState);
  const node = connection.nodes.find((entry) => entry.id === connection.selected);
  const phase = connection.sessions[connection.selected]?.phase ?? "unconfigured";
  const label = `${node?.label ?? "Node"}: ${phase}. Open connections`;
  return (
    <NodeMenu
      nodes={connection.nodes}
      selected={connection.selected}
      selectionLabel="Default node"
      onSelect={selectNode}
      placement="top start"
    >
      {(open) => (
        <ChromeButton
          size="rail"
          aria-label={label}
          title={label}
          selected={open}
          className="relative"
        >
          <span className="relative flex size-(--spacing-chrome-rail-icon) items-center justify-center">
            <Monitor className="size-(--spacing-chrome-rail-icon) shrink-0" />
            <span
              aria-hidden="true"
              className={`absolute -right-0.5 -bottom-0.5 size-1.5 rounded-full ${phase === "connected" ? "bg-success" : "bg-danger"}`}
            />
          </span>
        </ChromeButton>
      )}
    </NodeMenu>
  );
}
