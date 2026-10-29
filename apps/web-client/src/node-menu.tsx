import { useSource } from "@swarmie/client-ui";
import type { Node } from "@swarmie/plugin-connections-browser";
import { ActionMenuDialog, Popover } from "@swarmie/ui-web";
import { Monitor, Settings } from "lucide-react";
import type { ReactNode } from "react";
import { useState } from "react";

import { useClient } from "./client/context";
import { Caption, ChoiceList, Item } from "./controls";

export function NodeMenu({
  nodes,
  selected,
  selectionLabel,
  onSelect,
  placement,
  children,
}: {
  nodes: readonly Node[];
  selected: string;
  selectionLabel: string;
  onSelect: (id: string) => void;
  placement: "top start" | "top end";
  children: (open: boolean) => ReactNode;
}) {
  const { connectionState, navigate, shellState } = useClient();
  const connection = useSource(connectionState);
  const [open, setOpen] = useState(false);
  return (
    <Popover isOpen={open} onOpenChange={setOpen}>
      {children(open)}
      <Popover.Content placement={placement} className="z-50">
        <ActionMenuDialog aria-label="Connections">
          <Caption>Nodes</Caption>
          <ChoiceList
            label={selectionLabel}
            value={selected}
            items={nodes.map((node) => ({
              id: node.id,
              label: node.label,
              description: connection.sessions[node.id]?.phase ?? "unconfigured",
              icon: Monitor,
            }))}
            onAction={(id) => {
              setOpen(false);
              onSelect(id);
            }}
          />
          <div className="my-1 border-t border-border" />
          <Item
            icon={Settings}
            onClick={() => {
              setOpen(false);
              navigate({ section: "settings", settings: "connections" });
              shellState.update((current) => ({
                ...current,
                navigation: window.innerWidth >= 768,
              }));
            }}
          >
            Manage connections
          </Item>
        </ActionMenuDialog>
      </Popover.Content>
    </Popover>
  );
}
