import { useSource } from "@swarmie/client-ui";

import { useClient } from "../client/context";
import { ConnectionSettings } from "./connections";
import { GeneralSettings } from "./general";
import { PluginSettings } from "./plugins";
import { ProviderSettings } from "./providers";

function SettingsContent({ section }: { section: string }) {
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
      className="min-h-0 flex-1 overflow-auto bg-background px-6 py-10 md:px-12 md:py-14"
    >
      <div className="mx-auto max-w-2xl">
        <SettingsContent section={layout.settings} />
      </div>
    </section>
  );
}
