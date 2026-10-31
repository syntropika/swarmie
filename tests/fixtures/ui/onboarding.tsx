import { Button } from "@swarmie/ui-web";
import { setThemePreference } from "@swarmie/ui-web/theme";
import { useState } from "react";
import { createRoot } from "react-dom/client";

import type { UiStore } from "../../../packages/client-ui/src/index";
import { OnboardingGuide } from "../../../plugins/browser/src/onboarding/views";
import { createProvidersApi, providerCatalog } from "../../../plugins/providers/browser/src/index";
import type { ProvidersState } from "../../../plugins/providers/browser/src/index";
import { ProvidersPanel } from "../../../plugins/providers/browser/src/views";

import "../../../apps/web-client/src/styles.css";

let value: Readonly<ProvidersState> = {
  scope: null, ready: true,
  error: null,
  accounts: [
    {
      id: "example-account",
      providerId: "openai",
      providerLabel: "OpenAI",
      label: "Example account",
      available: true,
      models: [{ id: "balanced", label: "Balanced model", efforts: ["Low", "Medium", "High"] }],
    },
  ],
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
    for (const listener of listeners) {
      listener();
    }
  },
  update: (change) => {
    state.set(change(value));
  },
};
const providers = createProvidersApi(
  state,
  async (change) => {
    state.update(change);
  },
  {
    catalog: { getSnapshot: state.getSnapshot, subscribe: () => () => undefined },
    configuration: {
      configure: async ({ providerId }) => ({
        id: "configured-example",
        providerId,
        providerLabel:
          value.choices.find((choice) => choice.id === providerId)?.label ?? providerId,
        label: "Configured example account",
        available: true,
        models: [{ id: "balanced", label: "Balanced model", efforts: [] }],
      }),
    },
  },
);
function Fixture() {
  const [open, setOpen] = useState(false);
  return (
    <main className="min-h-dvh bg-pane p-4 text-foreground">
      <p className="mb-4 text-sm text-muted">
        Synthetic onboarding. No live credentials, accounts or services.
      </p>
      <div className="flex flex-wrap gap-3">
        <Button size="sm" variant="secondary" onPress={() => setThemePreference("light")}>
          Light
        </Button>
        <Button size="sm" variant="secondary" onPress={() => setThemePreference("dark")}>
          Dark
        </Button>
        <Button
          size="sm"
          onPress={() => {
            setOpen(true);
          }}
        >
          Open onboarding
        </Button>
      </div>
      {open ? (
        <OnboardingGuide
          onComplete={() => {
            setOpen(false);
          }}
          renderProviderConfiguration={({ onConfigured, onSelectionChange }) => (
            <ProvidersPanel
              providers={providers}
              onConfigured={onConfigured}
              onSelectionChange={(choice) => {
                onSelectionChange(choice !== null);
              }}
            />
          )}
        />
      ) : null}
    </main>
  );
}
const element = document.getElementById("root");
if (element === null) {
  throw new Error("The fixture root is missing.");
}
const root = createRoot(element);
root.render(<Fixture />);
import.meta.hot?.dispose(() => {
  root.unmount();
  providers.dispose();
});
