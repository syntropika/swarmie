import { ChromeButton } from "@swarmie/ui-web";
import { Monitor, Puzzle, Settings } from "lucide-react";

import { useClient } from "../../client/context";
import { useSelection } from "../../client/use-selection";
import { iconStyle } from "../../controls";
import { shellSettings } from "./state";

export function SettingsNavigation() {
  const { navigate, shell, shellState } = useClient();
  const selected = useSelection(shell, shellSettings);
  return (
    <>
      <div className="flex h-9 shrink-0 items-center border-b border-border px-3 text-xs font-medium">
        Settings
      </div>
      <div className="space-y-1 p-2 pt-4">
        {(
          [
            ["general", Settings, "General"],
            ["connections", Monitor, "Connections"],
            ["providers", Puzzle, "Providers"],
            ["plugins", Puzzle, "Plugins"],
          ] as const
        ).map(([id, Icon, title]) => (
          <ChromeButton
            key={id}
            size="navigation"
            selected={selected === id}
            onPress={() => {
              navigate({ settings: id });
              if (window.innerWidth < 768) {
                shellState.update((s) => ({ ...s, navigation: false }));
              }
            }}
          >
            <Icon className={iconStyle} />
            {title}
          </ChromeButton>
        ))}
      </div>{" "}
    </>
  );
}
