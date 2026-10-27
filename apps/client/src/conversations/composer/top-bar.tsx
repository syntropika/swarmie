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
    <div className="flex h-8 items-center justify-between gap-2 border-b border-border bg-composer-context px-2 text-chrome text-muted">
      {(["left", "right"] as const).map((side) => (
        <div
          key={side}
          className={`flex min-w-0 items-center gap-0.5 ${side === "right" ? "ml-auto shrink-0 flex-row-reverse" : "overflow-x-auto"}`}
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
