import type { Conversation } from "@swarmie/plugin-conversations-browser";
import { Archive, Pin } from "lucide-react";

import { useClient } from "../../client/context";
import { IconButton } from "../../controls";

export function ConversationRow({ chat, active }: { chat: Conversation; active: boolean }) {
  const { archive, openConversation, shellState, togglePin } = useClient();
  return (
    <div
      className={`group flex min-h-8 items-center rounded-md ${active ? "bg-row-selected" : "hover:bg-row-hover"}`}
    >
      <button
        type="button"
        onClick={() => {
          openConversation(chat.id);
          if (window.innerWidth < 768) {
            shellState.update((s) => ({ ...s, navigation: false }));
          }
        }}
        className="min-w-0 flex-1 truncate px-2 py-2 text-left text-xs"
      >
        {chat.title}
      </button>
      <div className="mr-1 hidden items-center group-focus-within:flex group-hover:flex">
        <IconButton
          icon={Pin}
          size="tabClose"
          label={`Pin ${chat.title}`}
          active={chat.pinned}
          onPress={() => {
            togglePin(chat);
          }}
        />
        <IconButton
          icon={Archive}
          size="tabClose"
          label={`Archive ${chat.title}`}
          onPress={() => {
            archive(chat);
          }}
        />
      </div>
    </div>
  );
}
