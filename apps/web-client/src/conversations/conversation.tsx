import { useSource } from "@swarmie/client-ui";
import type { Conversation } from "@swarmie/plugin-conversations-browser";
import type { VirtualListHandle } from "@swarmie/ui-web";
import { useRef } from "react";

import { VaultCards } from "../vault/cards";
import { Composer } from "./composer/composer";
import { Transcript } from "./transcript";

export function ConversationPanel({ chat }: { chat: Conversation }) {
  const list = useRef<VirtualListHandle>(null);
  const execution = useSource(chat.execution);
  return (
    <section
      aria-label="Conversation"
      className="flex h-full min-h-0 min-w-0 flex-col overflow-hidden bg-pane shadow-sheet"
    >
      <Transcript key={`transcript-${chat.id}`} chat={chat} list={list} />
      <VaultCards conversation={execution.ownerConversationId} />
      <Composer
        key={`composer-${chat.id}`}
        chat={chat}
        showStatus={() => list.current?.scrollToKey("activity")}
      />
    </section>
  );
}
