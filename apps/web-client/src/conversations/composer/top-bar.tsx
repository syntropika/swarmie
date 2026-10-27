import { useSource } from "@swarmie/client-ui";
import type { ComposerTopBarItem, Conversation } from "@swarmie/plugin-conversations-browser";

import { useClient } from "../../client/context";
import { ComposerAccount } from "./account";
import { ComposerNode } from "./node";

const baseTopBar: readonly ComposerTopBarItem[] = [
  {
    id: "swarmie.providers.account",
    owner: "providers",
    side: "left",
    priority: 100,
    component: ComposerAccount,
  },
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
    <div className="flex min-h-(--spacing-chrome-menu-row) flex-wrap items-center justify-between gap-(--spacing-control-group) border-b border-border bg-composer-context px-(--spacing-chrome-inset) text-chrome text-muted">
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
    </div>
  );
}
