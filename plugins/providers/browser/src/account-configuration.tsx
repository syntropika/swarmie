import { Button, ProviderLogo } from "@swarmie/ui-web";

import type { ModelDefaultsApi, ProviderAccount } from "./index";
import { ModelDefaultsSettings } from "./model-defaults";
import { providerDisplayName } from "./presentation";
import { ThinkingMenu } from "./thinking-menu";

/** Configuration reads the current public account projection without inventing writable settings. */
export function AccountConfiguration({
  account,
  onReconnect,
  defaults,
}: {
  account: ProviderAccount | undefined;
  onReconnect?: (() => void) | undefined;
  defaults: ModelDefaultsApi | null;
}) {
  return (
    <section aria-label="Account configuration">
      {account === undefined ? (
        <p className="text-sm text-muted">This account is no longer available.</p>
      ) : (
        <div className="@container grid gap-7">
          <header className="flex min-w-0 flex-wrap items-center justify-between gap-4 rounded-(--radius-group) border border-border p-4">
            <div className="flex min-w-0 items-center gap-3">
              <ProviderLogo providerId={account.providerId} className="size-6" />
              <div className="grid min-w-0 gap-1">
                <h2 className="text-sm font-medium">
                  {providerDisplayName(account.providerId, account.providerLabel)}
                </h2>
                <p className="text-sm wrap-anywhere">{account.label}</p>
                <p className="text-xs wrap-anywhere text-muted">Account ID: {account.id}</p>
              </div>
            </div>
            {account.authorizationMethod === "oauth-browser" && onReconnect !== undefined ? (
              <Button size="sm" variant="secondary" onPress={onReconnect}>
                Reconnect account
              </Button>
            ) : null}
          </header>
          <ModelDefaultsSettings key={account.id} account={account} api={defaults} />
          <section aria-label="Available models" className="grid gap-3">
            <h2 className="text-sm font-medium">Available models</h2>
            {account.models.length === 0 ? (
              <p className="text-sm text-muted">No models are available for this account.</p>
            ) : (
              <div className="overflow-hidden rounded-(--radius-group) border border-border">
                <div
                  aria-hidden="true"
                  className="hidden grid-cols-[minmax(0,1fr)_minmax(0,1.5fr)] gap-6 bg-group px-4 py-3 text-xs text-muted @min-[600px]:grid"
                >
                  <span>Model</span>
                  <span>Supported thinking</span>
                </div>
                <ul className="divide-y divide-border">
                  {account.models.map((model) => (
                    <li
                      key={model.id}
                      className="grid min-w-0 items-center gap-3 p-4 @min-[600px]:grid-cols-[minmax(0,1fr)_minmax(0,1.5fr)] @min-[600px]:gap-6"
                    >
                      <div className="grid min-w-0 gap-1">
                        <span className="text-sm wrap-anywhere">{model.label}</span>
                        {model.description !== undefined && model.description !== "" ? (
                          <p className="text-xs text-muted">{model.description}</p>
                        ) : null}
                      </div>
                      <ThinkingMenu providerId={account.providerId} model={model} />
                    </li>
                  ))}
                </ul>
              </div>
            )}
          </section>
        </div>
      )}
    </section>
  );
}
