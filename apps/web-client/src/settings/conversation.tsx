import { useSource } from "@swarmie/client-ui";
import { isToolDetails } from "@swarmie/plugin-settings-browser";
import { Tabs } from "@swarmie/ui-web";

import { useClient } from "../client/context";

const modeLabels = { grouped: "Grouped", separate: "Separate", hidden: "Hidden" };

export function ConversationSettings() {
  const { settings } = useClient();
  const preferences = useSource(settings);
  return (
    <>
      <h1 className="text-2xl font-medium">Conversation</h1>
      <div className="mt-8 grid min-w-0 gap-3 rounded-group bg-group p-4">
        <h2 className="text-sm">Tool details</h2>
        <p id="tool-details-description" className="text-caption text-muted">
          Choose how tool results appear. Messages, request status and recovery notices stay
          visible.
        </p>
        <Tabs
          selectedKey={preferences.toolDetails}
          onSelectionChange={(value) => {
            if (isToolDetails(value)) {
              settings.update((current) => ({ ...current, toolDetails: value }));
            }
          }}
        >
          <Tabs.ListContainer>
            <Tabs.List aria-label="Tool details" aria-describedby="tool-details-description">
              {(["grouped", "separate", "hidden"] as const).map((mode) => (
                <Tabs.Tab key={mode} id={mode}>
                  {modeLabels[mode]}
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
