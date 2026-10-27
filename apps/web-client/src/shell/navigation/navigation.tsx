import { useState } from "react";

import { useClient } from "../../client/context";
import { useSelection } from "../../client/use-selection";
import { ResizeHandle } from "../resize";
import { NavigationRail } from "./rail";
import { SettingsNavigation } from "./settings";
import { ConversationSidebar } from "./sidebar";
import { navigationLocation, sameNavigationLocation } from "./state";
import type { NavigationPreferences } from "./state";

export function Navigation() {
  const { shell, shellState } = useClient();
  const layout = useSelection(shell, navigationLocation, sameNavigationLocation);
  const [mode, setMode] = useState<NavigationPreferences["mode"]>("chats");
  const [folded, setFolded] = useState<string[]>([]);
  const hidden = !layout.navigation;
  const preferences: NavigationPreferences = {
    mode,
    folded,
    setMode,
    toggleGroup: (id) => {
      setFolded((old) => (old.includes(id) ? old.filter((entry) => entry !== id) : [...old, id]));
    },
  };
  return (
    <>
      {layout.navigation ? (
        <button
          aria-label="Close navigation"
          type="button"
          onClick={() => {
            shellState.update((current) => ({ ...current, navigation: false }));
          }}
          className="fixed inset-0 z-20 bg-background/60 md:hidden"
        />
      ) : null}
      <div
        className={`z-30 flex shrink-0 border-r border-border bg-navigation-surface ${hidden ? "hidden md:flex" : "fixed inset-y-11 left-0 md:static"}`}
      >
        <NavigationRail hidden={hidden} />
        {hidden ? null : (
          <aside
            id="client-navigation"
            className={`flex w-(--navigation-width) flex-col overflow-hidden ${layout.section === "conversations" ? "bg-conversation-navigation" : ""}`}
          >
            {layout.section === "settings" ? (
              <SettingsNavigation />
            ) : (
              <ConversationSidebar preferences={preferences} />
            )}
          </aside>
        )}
      </div>
      {layout.navigation ? <ResizeHandle target="navigation" /> : null}
    </>
  );
}
