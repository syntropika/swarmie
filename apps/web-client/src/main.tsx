import { browserProjectsPersistence } from "@swarmie/plugin-projects-browser/persistence";
import { browserToolDetailsPersistence } from "@swarmie/plugin-settings-browser";
import { ApplicationSkeleton } from "@swarmie/ui-web";
import { getThemePreferenceSnapshot, subscribeThemePreference } from "@swarmie/ui-web/theme";
import { createHost } from "@syntropika/tembra-sdk/browser";
import { createRoot } from "react-dom/client";

import { browserNode } from "./browser-node";
import { ClientContext } from "./client/context";
import { initializeClient } from "./client/initialize";
import { nativeVault } from "./client/native-vault";
import { desktopNode } from "./desktop-node";
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
  const desktop = await desktopNode().catch((error: unknown) => {
    reportFailure(error);
    return null;
  });
  const browser =
    desktop === null
      ? await browserNode(window.location.href).catch((error: unknown) => {
          reportFailure(error);
          return null;
        })
      : null;
  const initialNode = desktop?.node ?? browser;
  const client = await initializeClient(host, {
    ...(initialNode === null ? {} : { nodes: [initialNode] }),
    ...(desktop === null ? {} : { connector: desktop.connector }),
    dev: import.meta.env.DEV,
    nodeServices: true,
    projectsPersistence: browserProjectsPersistence("local"),
    toolDetailsPersistence: browserToolDetailsPersistence(),
  });
  client.settings.update((current) => ({ ...current, theme: getThemePreferenceSnapshot() }));
  const stopTheme = subscribeThemePreference(() => {
    const theme = getThemePreferenceSnapshot();
    client.settings.update((current) =>
      current.theme === theme ? current : { ...current, theme },
    );
  });
  root.render(
    <ClientContext value={client}>
      <App />
    </ClientContext>,
  );
  // Local presentation remains usable while the private native owner opens its protected store.
  nativeVault()
    .then((backend) => {
      if (backend) {
        client.vault.attach(backend);
      }
    })
    .catch(() => undefined);
  function stop() {
    stopTheme();
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
