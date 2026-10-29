import type { UiStore } from "../../../packages/client-ui/src/index";
import { Button } from "@swarmie/ui-web";
import { useState } from "react";
import { createRoot } from "react-dom/client";

import { OnboardingGuide } from "../../../plugins/onboarding/browser/src/views";
import { createProvidersApi, providerCatalog } from "../../../plugins/providers/browser/src/index";
import type { ProvidersState } from "../../../plugins/providers/browser/src/index";
import { OAuthCallback } from "../../../plugins/providers/browser/src/oauth-callback";

import "../../../apps/web-client/src/styles.css";
import { ProvidersPanel } from "../../../plugins/providers/browser/src/views";

const catalog: ProvidersState = {
  scope: null, ready: true,
  error: null,
  accounts: [],
  choices: providerCatalog(),
};
const state: UiStore<Readonly<ProvidersState>> = {
  getSnapshot: () => catalog,
  subscribe: () => () => undefined,
  set: () => undefined,
  update: () => undefined,
};
const providers = createProvidersApi(state, async () => undefined, {
  catalog: state,
  configuration: {
    configure: async () => {
      throw new Error("Synthetic fixture: no account is created.");
    },
  },
});
function Fixture() {
  const [guide, setGuide] = useState(false);
  return (
    <main className="min-h-dvh bg-background p-4 text-foreground">
      <p className="mb-4 text-caption text-muted">
        Synthetic presentation. No credentials or live services.
      </p>
      <div className="mx-auto grid max-w-2xl gap-6">
        <Button
          onPress={() => {
            setGuide(true);
          }}
        >
          Open synthetic onboarding
        </Button>
        <ProvidersPanel providers={providers} />
        <OAuthCallback
          callbackUri="https://example.invalid/callback"
          onComplete={async () => {
            throw new Error("Synthetic callback error.");
          }}
        />
      </div>
      {guide ? (
        <OnboardingGuide
          renderProviderConfiguration={({ onConfigured, onSelectionChange }) => (
            <ProvidersPanel
              providers={providers}
              onConfigured={onConfigured}
              onSelectionChange={(choice) => {
                onSelectionChange(choice !== null);
              }}
            />
          )}
          onComplete={() => {
            setGuide(false);
          }}
        />
      ) : null}
    </main>
  );
}
const root = document.getElementById("root");
if (root !== null) {
  const renderer = createRoot(root);
  renderer.render(<Fixture />);
  import.meta.hot?.dispose(() => {
    renderer.unmount();
  });
}
