import type { Conversation } from "@swarmie/plugin-conversations-browser";
import { Archive, Pin } from "lucide-react";

import { useClient } from "../../client/context";
import { IconButton } from "../../controls";

export function ConversationRow({ chat, active }: { chat: Conversation; active: boolean }) {
  const { archive, openConversation, shellState, togglePin } = useClient();
  return (
    <div
      className={`group flex min-h-(--spacing-chrome-navigation-row) items-center rounded-md ${active ? "bg-navigation-selected" : "hover:bg-row-hover"}`}
    >
      <button
        type="button"
        aria-current={active ? "page" : undefined}
        onClick={() => {
          openConversation(chat.id);
          if (window.innerWidth < 768) {
            shellState.update((s) => ({ ...s, navigation: false }));
          }
        }}
        className={`min-h-(--spacing-chrome-navigation-row) min-w-0 flex-1 truncate px-(--spacing-navigation-row-x) py-(--spacing-navigation-row-y) text-left text-navigation ${active ? "font-medium" : ""}`}
      >
        {chat.title}
      </button>
      <div className="mr-1 hidden items-center group-focus-within:flex group-hover:flex [@media(hover:none)]:flex">
        <IconButton
          icon={Pin}
          size="tabClose"
          label={`Pin ${chat.title}`}
          active={chat.pinned}
          aria-pressed={chat.pinned}
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
