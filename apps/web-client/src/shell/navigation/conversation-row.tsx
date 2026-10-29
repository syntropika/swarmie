import type { Conversation } from "@swarmie/plugin-conversations-browser";
import { ChromeButton } from "@swarmie/ui-web";
import { Archive, Pin } from "lucide-react";

import { useClient } from "../../client/context";
import { IconButton } from "../../controls";

export function ConversationRow({ chat, active }: { chat: Conversation; active: boolean }) {
  const { archive, openConversation, shellState, togglePin } = useClient();
  return (
    <div className="group relative my-0.5 flex min-h-(--spacing-chrome-navigation-row) items-center">
      <ChromeButton
        size="navigation"
        selected={active}
        aria-current={active ? "page" : false}
        onPress={() => {
          openConversation(chat.id);
          if (window.innerWidth < 768) {
            shellState.update((s) => ({ ...s, navigation: false }));
          }
        }}
        className="min-w-0 flex-1"
      >
        <span className="truncate">{chat.title}</span>
      </ChromeButton>
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
