import { useSource } from "@swarmie/client-ui";
import { ChromeButton } from "@swarmie/ui-web";
import { Activity, Plus } from "lucide-react";

import { useClient } from "../../client/context";
import { IconButton, iconStyle } from "../../controls";
import { ActivityNavigation } from "./activity";
import { ConversationsNavigation } from "./conversations";
import type { NavigationPreferences } from "./state";

export function ConversationSidebar({ preferences }: { preferences: NavigationPreferences }) {
  const { createConversation, plugins, shellState } = useClient();
  const enabled = useSource(plugins);
  const activityEnabled = enabled.some((plugin) => plugin.id === "activity" && plugin.enabled);
  return (
    <>
      <div className="flex min-h-(--spacing-navigation-header) shrink-0 items-center gap-0.5 border-b border-navigation-divider bg-navigation-surface px-1.5">
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
        {activityEnabled ? (
          <IconButton
            icon={Activity}
            label="Toggle activity"
            active={preferences.mode === "activity"}
            aria-pressed={preferences.mode === "activity"}
            onPress={() => {
              preferences.setMode(preferences.mode === "chats" ? "activity" : "chats");
            }}
          />
        ) : null}
      </div>
      <div className="min-h-0 flex-1 overflow-auto p-2">
        {preferences.mode === "activity" && activityEnabled ? (
          <ActivityNavigation
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
