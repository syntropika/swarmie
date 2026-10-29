import { useSource } from "@swarmie/client-ui";
import { Tabs } from "@swarmie/ui-web";
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
      <div className="mt-8 rounded-group bg-group p-4">
        <h2 className="text-sm">Appearance</h2>
        <Tabs
          selectedKey={preferences.theme}
          onSelectionChange={(theme) => {
            if (theme === "system" || theme === "dark" || theme === "light") {
              settings.update((current) => ({ ...current, theme }));
              setThemePreference(theme);
            }
          }}
          className="mt-3"
        >
          <Tabs.ListContainer>
            <Tabs.List aria-label="Appearance">
              {(["system", "dark", "light"] as const).map((theme) => (
                <Tabs.Tab key={theme} id={theme}>
                  {themeLabel(theme)}
                  <Tabs.Indicator />
                </Tabs.Tab>
              ))}
            </Tabs.List>
          </Tabs.ListContainer>
        </Tabs>
      </div>
    </>
  );
}
