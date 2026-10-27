import { useSource } from "@swarmie/client-ui";
import type { Conversation } from "@swarmie/plugin-conversations-browser";

import { useClient } from "../../client/context";
import { useSelection } from "../../client/use-selection";
import { ConversationRow } from "./conversation-row";
import { NavigationGroup } from "./group";
import { activeConversation } from "./state";
import type { NavigationPreferences } from "./state";

export function ConversationsNavigation({ preferences }: { preferences: NavigationPreferences }) {
  const { conversationsUi, openConversation, roster, shell } = useClient();
  const activeId = useSelection(shell, activeConversation);
  const chats = useSource(roster).filter((chat) => chat.started && !chat.archived);
  const groups = useSource(conversationsUi.navigation.groups);
  const grouped = new Set(groups.flatMap((group) => group.membership.getSnapshot()));
  const rows = (items: readonly Conversation[]) =>
    items.map((chat) => (
      <ConversationRow key={chat.id} chat={chat} active={activeId === chat.id} />
    ));
  return (
    <>
      {chats.some((chat) => chat.pinned) ? (
        <NavigationGroup
          title="Pinned"
          folded={preferences.folded.includes("pinned")}
          onToggle={() => {
            preferences.toggleGroup("pinned");
          }}
        >
          {rows(chats.filter((chat) => chat.pinned))}
        </NavigationGroup>
      ) : null}
      {groups.map(({ id, component: Group }) => (
        <Group
          key={id}
          conversations={chats}
          activeId={activeId}
          openConversation={openConversation}
          renderConversations={(items) => {
            const ids = new Set(items.map((item) => item.id));
            return rows(chats.filter((chat) => ids.has(chat.id)));
          }}
        />
      ))}
      <NavigationGroup
        title="Recent chats"
        folded={preferences.folded.includes("recent")}
        onToggle={() => {
          preferences.toggleGroup("recent");
        }}
      >
        {rows(chats.filter((chat) => !grouped.has(chat.id) && !chat.pinned))}
      </NavigationGroup>
    </>
  );
}
