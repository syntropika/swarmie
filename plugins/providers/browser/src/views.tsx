import { useSource } from "@swarmie/client-ui";
import {
  Button,
  ClaudeLogo,
  CopilotLogo,
  KimiLogo,
  LoadingRows,
  OpenAILogo,
} from "@swarmie/ui-web";
import { ArrowLeft, Monitor } from "lucide-react";
import { useState } from "react";

import type { ProviderAccount, ProviderChoice, ProvidersApi } from "./index";
import { ProviderMethodView } from "./method-view";

const marks = {
  openai: OpenAILogo,
  claude: ClaudeLogo,
  kimi: KimiLogo,
  "github-copilot": CopilotLogo,
  local: Monitor,
};
function ProviderMark({ id }: { id: string }) {
  const Mark = Object.entries(marks).find(([key]) => key === id)?.[1] ?? Monitor;
  return <Mark className="size-7" />;
}
export function ProvidersPanel({
  providers,
  onConfigured,
  onSelectionChange,
}: {
  providers: ProvidersApi;
  onConfigured?: (account: ProviderAccount) => void;
  onSelectionChange?: (provider: ProviderChoice | null) => void;
}) {
  const catalog = useSource(providers.state);
  const [selected, setSelected] = useState<string | null>(null);
  const [method, setMethod] = useState("");
  const [busy, setBusy] = useState(false);
  const provider = catalog.choices.find((entry) => entry.id === selected);
  const selectedMethod = provider?.methods.find((entry) => entry.id === method);
  const choose = (choice: ProviderChoice | null) => {
    setSelected(choice?.id ?? null);
    setMethod(choice?.methods[0]?.id ?? "");
    onSelectionChange?.(choice);
  };
  function connected(account: ProviderAccount) {
    onConfigured?.(account);
  }
  if (!catalog.ready && (providers.configuration !== null || providers.authorization !== null)) {
    return <LoadingRows label="Loading providers" rows={3} />;
  }
  if (provider) {
    return (
      <div className="grid gap-5">
        <Button
          variant="tertiary"
          className="justify-self-start"
          onPress={() => {
            choose(null);
          }}
          isDisabled={busy}
        >
          <ArrowLeft className="size-3.5" />
          Choose another provider
        </Button>
        <div className="flex items-center gap-3">
          <ProviderMark id={provider.id} />
          <div>
            <h2 className="text-lg font-medium">{provider.label}</h2>
            <p className="text-xs text-muted">By {provider.publisher}</p>
          </div>
        </div>
        {provider.methods.length > 1 ? (
          <label className="grid gap-2 text-sm">
            Connection method
            <select
              className="rounded-lg border border-border bg-default p-3"
              value={method}
              disabled={busy}
              onChange={(event) => {
                setMethod(event.target.value);
              }}
            >
              {provider.methods.map((entry) => (
                <option key={entry.id} value={entry.id}>
                  {entry.label}
                </option>
              ))}
            </select>
          </label>
        ) : null}
        {selectedMethod === undefined ? (
          <p className="text-sm text-muted">No connection method is available for this provider.</p>
        ) : (
          <ProviderMethodView
            key={`${provider.id}:${selectedMethod.id}`}
            providers={providers}
            provider={provider}
            method={selectedMethod}
            onConfigured={connected}
            onBusyChange={setBusy}
          />
        )}
      </div>
    );
  }
  return (
    <div className="@container/provider-configuration grid gap-5">
      <div className="grid grid-cols-2 gap-4 @min-[540px]/provider-configuration:grid-cols-3">
        {catalog.choices.map((choice) => (
          <div key={choice.id} className="overflow-hidden rounded-xl border border-border">
            <Button
              variant="tertiary"
              shape="tile"
              className="h-auto w-full justify-start text-left"
              onPress={() => {
                choose(choice);
              }}
            >
              <span className="flex min-h-20 items-center gap-3 p-4">
                <ProviderMark id={choice.id} />
                <span className="grid gap-1">
                  <span className="text-sm">{choice.label}</span>
                  <span className="text-chrome text-muted">By {choice.publisher}</span>
                </span>
              </span>
            </Button>
          </div>
        ))}
      </div>
      {catalog.choices.length === 0 ? (
        <p className="text-sm leading-6 text-muted">
          Connect an execution node to configure its AI providers.
        </p>
      ) : null}
      {catalog.accounts.length > 0 ? (
        <div className="divide-y divide-border rounded-xl border border-border px-4">
          {catalog.accounts.map((account) => (
            <div key={account.id} className="flex items-center gap-3 py-4">
              <ProviderMark id={account.providerId} />
              <div>
                <h3 className="text-sm">{account.label}</h3>
                <p className="text-xs text-muted">
                  {account.providerLabel} · {account.models.length} models
                </p>
              </div>
            </div>
          ))}
        </div>
      ) : null}
    </div>
  );
}
