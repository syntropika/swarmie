import { useSource } from "@swarmie/client-ui";
import { ChromeButton } from "@swarmie/ui-web";
import { setThemePreference } from "@swarmie/ui-web/theme";

import { useClient } from "../client/context";

function themeLabel(theme: string) {
  if (theme === "system") {
    return "System";
  }
  if (theme === "dark") {
    return "Dark";
  }
  return "Light";
}
export function GeneralSettings() {
  const { settings } = useClient();
  const preferences = useSource(settings);
  return (
    <>
      <h1 className="text-2xl font-medium">General</h1>
      <div className="mt-8 rounded-xl border border-border bg-chrome-surface p-4">
        <h2 className="text-sm">Appearance</h2>
        <fieldset aria-label="Appearance" className="mt-3 flex gap-2">
          {(["system", "dark", "light"] as const).map((theme) => (
            <ChromeButton
              key={theme}
              size="action"
              selected={preferences.theme === theme}
              aria-pressed={preferences.theme === theme}
              onPress={() => {
                settings.update((current) => ({ ...current, theme }));
                setThemePreference(theme);
              }}
            >
              {themeLabel(theme)}
            </ChromeButton>
          ))}
        </fieldset>
      </div>
    </>
  );
}
