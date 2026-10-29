import { createRoot } from "react-dom/client";
import { createHost } from "@syntropika/tembra-sdk/browser";
import type { ModelDefaults, ProviderAccount, ProvidersState } from "../../../plugins/providers/browser/src/index";
import { Button } from "@swarmie/ui-web";
import { setThemePreference } from "@swarmie/ui-web/theme";
import { ClientContext } from "../../../apps/web-client/src/client/context";
import { initializeClient } from "../../../apps/web-client/src/client/initialize";
import { ProviderSettings } from "../../../apps/web-client/src/settings/providers";
import { modelDefaults } from "../../../apps/web-client/src/client/provider-values";
import { ModelTrigger } from "../../../apps/web-client/src/conversations/composer/model-trigger";
import { useSource } from "../../../packages/client-ui/src/index";
import "../../../apps/web-client/src/styles.css";

const key = "swarmie.fixture.model-defaults";
const stored = localStorage.getItem(key);
const defaults: ModelDefaults = stored === null ? { model: "adaptive", effort: "balanced", auxiliaryModel: null } : modelDefaults(JSON.parse(stored));
const account: ProviderAccount = {
  id: "fixture-account", providerId: "openai", providerLabel: "OpenAI", label: "alex@example.com", available: true,
  defaults,
  models: [
    { id: "adaptive", label: "Adaptive fixture", efforts: ["quick", "balanced", "thorough"] },
    { id: "budget", label: "Budget fixture", efforts: ["1024 tokens", "4096 tokens", "16384 tokens"] },
    { id: "plain", label: "Plain fixture", efforts: [] },
    { id: "single", label: "Fixed fixture", efforts: ["deep"] },
  ],
};
let state: Readonly<ProvidersState> = { choices: [], accounts: [account], scope: null, ready: true, error: null };
const listeners = new Set<() => void>();
let rejectNext = false;
const client = await initializeClient(await createHost(), { dev: false, providers: {
  catalog: {
    getSnapshot: () => state,
    subscribe: (listener) => { listeners.add(listener); return () => { listeners.delete(listener); }; },
  },
  defaults: { save: async (_accountId, next) => {
    if (rejectNext) { rejectNext = false; throw new Error("Synthetic save failure. Your previous defaults are unchanged."); }
    localStorage.setItem(key, JSON.stringify(next));
    state = { ...state, accounts: [{ ...account, defaults: next }] };
    for (const listener of listeners) listener();
    return next;
  } },
} });
client.navigate({ section: "settings", settings: "providers", params: { providerAccount: account.id } });
setThemePreference(new URLSearchParams(location.search).get("theme") === "light" ? "light" : "dark");
function Fixture() {
  const providers = useSource(client.providers.state);
  const current = providers.accounts[0];
  const selection = client.providers.defaultSelection();
  const selected = current?.models.find((model) => model.id === selection?.model);
  return <ClientContext value={client}><main className="h-full overflow-auto bg-pane p-4 text-foreground md:p-10">
    <div className="mx-auto grid max-w-3xl gap-6">
      <nav aria-label="Fixture controls" className="flex items-center gap-3">
        <Button size="sm" variant="ghost" onPress={() => { rejectNext = true; }}>Reject next save</Button>
        <ModelTrigger providerId="openai" thinking={selection?.effort ?? ""} levels={selected?.efforts ?? []} open={false} label="Composer indicator fixture" />
      </nav>
      <ProviderSettings />
    </div>
  </main></ClientContext>;
}
const root = document.querySelector("#root");
if (root === null) throw new Error("The fixture root is missing.");
createRoot(root).render(<Fixture />);
