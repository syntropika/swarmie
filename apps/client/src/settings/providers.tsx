import { ProvidersPanel } from "@swarmie/plugin-providers-browser/views";

import { useClient } from "../client/context";

export function ProviderSettings() {
  const { providers } = useClient();
  return (
    <>
      <h1 className="mb-6 text-2xl font-medium">Providers</h1>
      <ProvidersPanel providers={providers} />
    </>
  );
}
