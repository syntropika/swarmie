import { browserProjectsPersistence } from "@swarmie/plugin-projects-browser/persistence";
import { ApplicationSkeleton } from "@swarmie/ui-web";
import { getThemePreferenceSnapshot } from "@swarmie/ui-web/theme";
import { createHost } from "@syntropika/tembra-sdk/browser";
import { createRoot } from "react-dom/client";

import { ClientContext } from "./client/context";
import { initializeClient } from "./client/initialize";
import { App } from "./shell/layout";

import "./styles.css";

const element = document.querySelector("#root");
if (element === null) {
  throw new Error("The application root is missing.");
}
const root = createRoot(element);
root.render(<ApplicationSkeleton />);

// The shell never waits for node networking or a capability inventory.
async function start() {
  const host = await createHost({
    name: "swarmie-client",
    onError: (failure) => {
      console.error(failure);
    },
  });
  const client = await initializeClient(host, {
    dev: import.meta.env.DEV,
    projectsPersistence: browserProjectsPersistence("local"),
  });
  client.settings.update((current) => ({ ...current, theme: getThemePreferenceSnapshot() }));
  root.render(
    <ClientContext value={client}>
      <App />
    </ClientContext>,
  );
  function stop() {
    root.unmount();
    client.close().catch(reportFailure);
  }
  import.meta.hot?.dispose(stop);
}
function reportFailure(error: unknown) {
  console.error(error);
}
function startupFailed(error: unknown) {
  console.error(error);
  root.render(<p role="alert">Swarmie could not start. Reload to try again.</p>);
}
start().catch(startupFailed);
