import { useSource } from "@swarmie/client-ui";
import { ChromeButton } from "@swarmie/ui-web";
import { Bell, Plus } from "lucide-react";

import { useClient } from "../../client/context";
import { IconButton, iconStyle } from "../../controls";
import { ConversationsNavigation } from "./conversations";
import { NotificationsNavigation } from "./notifications";
import type { NavigationPreferences } from "./state";

export function ConversationSidebar({ preferences }: { preferences: NavigationPreferences }) {
  const { createConversation, plugins, shellState } = useClient();
  const enabled = useSource(plugins);
  const notificationsEnabled = enabled.some(
    (plugin) => plugin.id === "notifications" && plugin.enabled,
  );
  return (
    <>
      <div className="flex min-h-(--spacing-navigation-header) shrink-0 items-center gap-0.5 border-b border-border px-1.5">
        <ChromeButton
          size="action"
          className="min-w-0 flex-1"
          onPress={() => {
            createConversation();
            if (window.innerWidth < 768) {
              shellState.update((current) => ({ ...current, navigation: false }));
            }
          }}
        >
          <Plus className={iconStyle} />
          <span className="truncate text-navigation-heading">New conversation</span>
        </ChromeButton>
        {notificationsEnabled ? (
          <IconButton
            icon={Bell}
            label="Toggle notifications"
            active={preferences.mode === "notifications"}
            aria-pressed={preferences.mode === "notifications"}
            onPress={() => {
              preferences.setMode(preferences.mode === "chats" ? "notifications" : "chats");
            }}
          />
        ) : null}
      </div>
      <div className="min-h-0 flex-1 overflow-auto px-2 py-4">
        {preferences.mode === "notifications" && notificationsEnabled ? (
          <NotificationsNavigation
            onClose={() => {
              preferences.setMode("chats");
            }}
          />
        ) : (
          <ConversationsNavigation preferences={preferences} />
        )}
      </div>
    </>
  );
}
