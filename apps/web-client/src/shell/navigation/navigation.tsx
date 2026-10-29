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
          className="fixed inset-x-(--spacing-shell-frame) top-0 bottom-[calc(var(--spacing-shell-frame)+var(--spacing-safe-area-bottom))] z-20 bg-background/60 md:hidden"
        />
      ) : null}
      <div
        className={`z-30 flex shrink-0 gap-(--spacing-workspace-inset) bg-workspace pr-(--spacing-workspace-inset) ${hidden ? "hidden md:flex" : "fixed top-[calc(var(--spacing-shell-bar)+var(--spacing-workspace-inset))] bottom-[calc(var(--spacing-shell-frame)+var(--spacing-workspace-inset)+var(--spacing-safe-area-bottom))] left-[calc(var(--spacing-shell-frame)+var(--spacing-workspace-inset))] md:static"}`}
      >
        <NavigationRail hidden={hidden} />
        {hidden ? null : (
          <aside
            id="client-navigation"
            className={`flex w-(--navigation-width) max-w-[calc(100dvw-var(--spacing-shell-rail)-var(--spacing-shell-frame)*2-var(--spacing-control-touch))] flex-col overflow-hidden rounded-l-(--radius-shell-frame) md:max-w-none ${layout.section === "conversations" ? "bg-conversation-navigation" : "bg-navigation-surface"}`}
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
