import { useSource } from "@swarmie/client-ui";
import type { ComposerTopBarItem, Conversation } from "@swarmie/plugin-conversations-browser";
import { ComposerPanel } from "@swarmie/ui-web/elements/conversation";

import { useClient } from "../../client/context";
import { ComposerNode } from "./node";

const baseTopBar: readonly ComposerTopBarItem[] = [
  {
    id: "swarmie.connections.node",
    owner: "connections",
    side: "right",
    priority: 100,
    component: ComposerNode,
  },
];

export function ComposerTopBar({ chat }: { chat: Conversation }) {
  const { conversationsUi } = useClient();
  const items = useSource(conversationsUi.composer.topBar.items);
  const topBar = [...baseTopBar, ...items];
  return (
    <ComposerPanel.Context>
      {(["left", "right"] as const).map((side) => (
        <div
          key={side}
          className={`flex max-w-full min-w-0 flex-wrap items-center gap-(--spacing-control-group) ${side === "right" ? "ml-auto flex-row-reverse" : ""}`}
        >
          {topBar
            .filter((item) => item.side === side)
            .toSorted((a, b) => b.priority - a.priority || a.id.localeCompare(b.id))
            .map(({ id, component: Control }) => (
              <Control key={id} conversationId={chat.id} isDraft={!chat.started} />
            ))}
        </div>
      ))}
    </ComposerPanel.Context>
  );
}
