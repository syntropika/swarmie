import { useSource } from "@swarmie/client-ui";
import { Button, Label, ListBox, LoadingRows, ProviderLogo, Select } from "@swarmie/ui-web";
import { ArrowLeft } from "lucide-react";
import { useState } from "react";

import type { ProviderAccount, ProviderChoice, ProvidersApi } from "./index";
import { ProviderMethodView } from "./method-view";
import { providerDisplayName } from "./presentation";

function ProviderMark({ id }: { id: string }) {
  return <ProviderLogo providerId={id} className="size-7" />;
}
export function ProvidersPanel({
  providers,
  onConfigured,
  onSelectionChange,
  initialProviderId,
  initialAccountId,
}: {
  providers: ProvidersApi;
  onConfigured?: (account: ProviderAccount) => void;
  onSelectionChange?: (provider: ProviderChoice | null) => void;
  initialProviderId?: string | undefined;
  initialAccountId?: string | undefined;
}) {
  const catalog = useSource(providers.state);
  const choices = catalog.choices.map((choice) => ({
    ...choice,
    label: providerDisplayName(choice.id, choice.label),
  }));
  const [selected, setSelected] = useState<string | null>(initialProviderId ?? null);
  const [method, setMethod] = useState(
    catalog.choices.find((entry) => entry.id === initialProviderId)?.methods[0]?.id ?? "",
  );
  const [busy, setBusy] = useState(false);
  const provider = choices.find((entry) => entry.id === selected);
  const selectedMethod = provider?.methods.find((entry) => entry.id === method);
  const choose = (choice: ProviderChoice | null) => {
    setSelected(choice?.id ?? null);
    setMethod(choice?.methods[0]?.id ?? "");
    onSelectionChange?.(choice);
  };
  function connected(account: ProviderAccount) {
    onConfigured?.(account);
  }
  if (
    !catalog.ready &&
    catalog.error === null &&
    (providers.configuration !== null || providers.authorization !== null)
  ) {
    return <LoadingRows label="Loading providers" rows={3} />;
  }
  if (provider) {
    return (
      <div className="grid gap-5">
        {catalog.error === null ? null : (
          <p role="alert" className="text-sm text-danger">
            {catalog.error}
          </p>
        )}
        <Button
          variant="ghost"
          className="justify-self-start"
          onPress={() => {
            choose(null);
          }}
          isDisabled={busy}
        >
          <ArrowLeft />
          Choose another provider
        </Button>
        <div className="flex items-center gap-3">
          <ProviderMark id={provider.id} />
          <div className="grid min-w-0 gap-1 wrap-anywhere">
            <h2 className="text-lg font-medium">{provider.label}</h2>
            {provider.label === provider.publisher ? null : (
              <p className="text-xs text-muted">By {provider.publisher}</p>
            )}
          </div>
        </div>
        {provider.methods.length > 1 ? (
          <Select
            value={method}
            isDisabled={busy}
            onChange={(value) => {
              if (typeof value === "string") {
                setMethod(value);
              }
            }}
          >
            <Label>Connection method</Label>
            <Select.Trigger>
              <Select.Value />
              <Select.Indicator />
            </Select.Trigger>
            <Select.Popover>
              <ListBox>
                {provider.methods.map((entry) => (
                  <ListBox.Item key={entry.id} id={entry.id} textValue={entry.label}>
                    {entry.label}
                    <ListBox.ItemIndicator />
                  </ListBox.Item>
                ))}
              </ListBox>
            </Select.Popover>
          </Select>
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
            accountId={provider.id === initialProviderId ? initialAccountId : undefined}
          />
        )}
      </div>
    );
  }
  return (
    <div className="@container/provider-choices grid gap-5">
      {catalog.error === null ? null : (
        <p role="alert" className="text-sm text-danger">
          {catalog.error}
        </p>
      )}
      <fieldset
        aria-label="Providers"
        className="grid min-w-0 grid-cols-1 gap-3 @min-[420px]/provider-choices:grid-cols-2 @min-[580px]/provider-choices:grid-cols-3"
      >
        {choices.map((choice) => (
          <Button
            key={choice.id}
            variant="outline"
            className="h-auto min-h-20 w-full min-w-0 justify-start text-left whitespace-normal"
            onPress={() => {
              choose(choice);
            }}
          >
            <ProviderMark id={choice.id} />
            <span className="grid min-w-0 gap-1 wrap-anywhere">
              <span>{choice.label}</span>
              {choice.label === choice.publisher ? null : (
                <span className="text-xs text-muted">By {choice.publisher}</span>
              )}
            </span>
          </Button>
        ))}
      </fieldset>
      {catalog.choices.length === 0 ? (
        <p className="text-sm leading-6 text-muted">
          Connect an execution node to configure its AI providers.
        </p>
      ) : null}
    </div>
  );
}
