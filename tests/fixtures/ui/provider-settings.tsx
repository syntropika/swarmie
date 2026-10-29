import { Button } from "@swarmie/ui-web";
import { setThemePreference } from "@swarmie/ui-web/theme";
import { createRoot } from "react-dom/client";
import { useState } from "react";

import type { UiStore } from "../../../packages/client-ui/src/index";
import { createProvidersApi, providerCatalog } from "../../../plugins/providers/browser/src/index";
import type { ProviderAccount, ProvidersState } from "../../../plugins/providers/browser/src/index";
import { ProviderSettingsPanel } from "../../../plugins/providers/browser/src/views";

import "../../../apps/web-client/src/styles.css";

const accounts: ProviderAccount[] = [
  {
    id: "synthetic-personal",
    providerId: "openai",
    providerLabel: "OpenAI",
    label: "Personal account",
    available: true,
    models: [
      { id: "balanced", label: "Balanced model", efforts: ["Low", "Medium", "High"] },
      { id: "fast", label: "Fast model", efforts: [] },
    ],
  },
  {
    id: "synthetic-work",
    providerId: "openai",
    providerLabel: "OpenAI",
    label: "Work account with a long name for narrow layouts",
    available: false,
    models: [{ id: "balanced", label: "Balanced model", efforts: ["Low", "High"] }],
  },
  {
    id: "synthetic-claude",
    providerId: "anthropic",
    providerLabel: "Claude",
    label: "Personal Claude account",
    available: true,
    models: [{ id: "balanced", label: "Balanced model", efforts: ["Low", "Medium", "High"] }],
  },
  {
    id: "synthetic-unlisted",
    providerId: "unlisted",
    providerLabel: "Unlisted provider",
    label: "Account retained outside the configuration catalog",
    available: false,
    models: [],
  },
];
let value: Readonly<ProvidersState> = {
  scope: null, ready: true,
  error: null,
  accounts,
  choices: providerCatalog(),
};
const listeners = new Set<() => void>();
const state: UiStore<Readonly<ProvidersState>> = {
  getSnapshot: () => value,
  subscribe: (changed) => {
    listeners.add(changed);
    return () => {
      listeners.delete(changed);
    };
  },
  set: (next) => {
    value = next;
    for (const listener of listeners) listener();
  },
  update: (change) => {
    state.set(change(value));
  },
};
let created = 0;
const providers = createProvidersApi(state, async (change) => {
  state.update(change);
}, {
    catalog: {
      getSnapshot: state.getSnapshot,
      subscribe: () => () => undefined,
    },
  configuration: {
    configure: async ({ providerId }) => {
      created += 1;
      const choice = value.choices.find((entry) => entry.id === providerId);
      return {
        id: `synthetic-created-${created}`,
        providerId,
        providerLabel: choice?.label ?? providerId,
        label: "New synthetic account",
        models: [],
        available: true,
      };
    },
  },
});

function Fixture() {
  const [accountId, setAccountId] = useState<string | undefined>();
  return (
    <main className="min-h-dvh bg-pane p-4 text-foreground">
      <div className="mx-auto grid max-w-3xl gap-6">
        <p className="text-sm text-muted">Synthetic provider data. No credentials or live services.</p>
        <div className="flex flex-wrap gap-2">
          <Button size="sm" variant="secondary" onPress={() => setThemePreference("light")}>Light</Button>
          <Button size="sm" variant="secondary" onPress={() => setThemePreference("dark")}>Dark</Button>
        </div>
        <h1 className="text-2xl font-medium">Providers</h1>
        <ProviderSettingsPanel providers={providers} accountId={accountId} onConfigure={setAccountId} />
      </div>
    </main>
  );
}
const element = document.getElementById("root");
if (element === null) throw new Error("The fixture root is missing.");
const root = createRoot(element);
root.render(<Fixture />);
import.meta.hot?.dispose(() => {
  root.unmount();
  providers.dispose();
});
