import { useSource } from "@swarmie/client-ui";

import { useClient } from "../client/context";
import { GitSettingsPage } from "../git/workspace";
import { ConnectionSettings } from "./connections";
import { ConversationSettings } from "./conversation";
import { GeneralSettings } from "./general";
import { PluginSettings } from "./plugins";
import { ProviderSettings } from "./providers";

function SettingsContent({ section }: { section: string }) {
  if (section === "conversation") {
    return <ConversationSettings />;
  }
  if (section === "git") {
    return <GitSettingsPage />;
  }
  if (section === "plugins") {
    return <PluginSettings />;
  }
  if (section === "providers") {
    return <ProviderSettings />;
  }
  if (section === "connections") {
    return <ConnectionSettings />;
  }
  return <GeneralSettings />;
}
export function SettingsPage() {
  const { shell } = useClient();
  const layout = useSource(shell);
  return (
    <section
      aria-label="Settings"
      className="min-h-0 min-w-0 flex-1 overflow-auto bg-pane px-panel-inset py-6 shadow-sheet md:px-12 md:py-14"
    >
      <div
        className={
          layout.settings === "plugins" || layout.settings === "providers"
            ? "mx-auto max-w-3xl"
            : "mx-auto max-w-2xl"
        }
      >
        <SettingsContent section={layout.settings} />
      </div>
    </section>
  );
}
