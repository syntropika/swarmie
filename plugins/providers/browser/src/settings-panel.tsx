import { useSource } from "@swarmie/client-ui";
import { Button, LoadingRows, ResponsiveDialog, SearchField, Surface } from "@swarmie/ui-web";
import { Plus } from "lucide-react";
import { useState } from "react";

import { AccountConfiguration } from "./account-configuration";
import { ProvidersPanel } from "./connection-panel";
import type { ProvidersApi } from "./index";
import { providerDisplayName } from "./presentation";
import { OtherProviders, ProviderAccounts } from "./provider-accounts";
import type { ProviderGroup } from "./provider-accounts";
import { ProviderFilter } from "./provider-filter";

/** Settings filters the live public catalog; connection continues through the shared flow. */
export function ProviderSettingsPanel({
  providers,
  accountId,
  onConfigure,
}: {
  providers: ProvidersApi;
  accountId: string | undefined;
  onConfigure: (id: string) => void;
}) {
  const catalog = useSource(providers.state);
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState<ReadonlySet<string> | null>(null);
  const [connection, setConnection] = useState<{ providerId?: string; accountId?: string } | null>(
    null,
  );
  const groups: ProviderGroup[] = catalog.choices.map((choice) => ({
    id: choice.id,
    label: providerDisplayName(choice.id, choice.label),
    choice,
    accounts: catalog.accounts.filter((account) => account.providerId === choice.id),
  }));
  for (const account of catalog.accounts) {
    if (!groups.some((group) => group.id === account.providerId)) {
      groups.push({
        id: account.providerId,
        label: providerDisplayName(account.providerId, account.providerLabel),
        choice: undefined,
        accounts: catalog.accounts.filter((entry) => entry.providerId === account.providerId),
      });
    }
  }
  const terms = query.trim().toLocaleLowerCase().split(/\s+/u).filter(Boolean);
  const matches = (text: string) => terms.every((term) => text.toLocaleLowerCase().includes(term));
  const filtered = groups
    .filter((group) => selected === null || selected.has(group.id))
    .flatMap((group) => {
      if (matches(`${group.label} ${group.id}`)) {
        return [group];
      }
      const accounts = group.accounts.filter((account) =>
        matches(`${group.label} ${group.id} ${account.label}`),
      );
      return accounts.length === 0 ? [] : [{ ...group, accounts }];
    });
  const connected = filtered.filter((group) => group.accounts.length > 0);
  const other = filtered.filter((group) => group.accounts.length === 0);
  if (
    !catalog.ready &&
    catalog.error === null &&
    (providers.configuration !== null || providers.authorization !== null)
  ) {
    return <LoadingRows label="Loading providers" rows={3} />;
  }
  return (
    <div className="@container grid gap-5">
      {accountId === undefined ? (
        <>
          <header className="flex flex-wrap items-start justify-between gap-4">
            <p className="max-w-md text-sm text-muted">
              Manage provider accounts and models on the selected execution node.
            </p>
            <Button
              size="sm"
              variant="secondary"
              isDisabled={catalog.choices.length === 0}
              onPress={() => {
                setConnection({});
              }}
            >
              <Plus />
              Connect provider
            </Button>
          </header>
          {catalog.error === null ? null : (
            <p role="alert" className="text-sm text-danger">
              {catalog.error}
            </p>
          )}
          <div className="flex flex-col gap-3 @min-[560px]:flex-row">
            <SearchField
              aria-label="Search providers"
              value={query}
              onChange={setQuery}
              className="min-w-0 flex-1"
            >
              <SearchField.Group>
                <SearchField.SearchIcon />
                <SearchField.Input placeholder="Search providers or accounts" />
                <SearchField.ClearButton />
              </SearchField.Group>
            </SearchField>
            <ProviderFilter options={groups} selected={selected} onChange={setSelected} />
          </div>
          <div className="grid gap-4">
            {connected.map((group) => (
              <ProviderAccounts
                key={group.id}
                group={group}
                connect={() => {
                  setConnection({ providerId: group.id });
                }}
                configure={onConfigure}
              />
            ))}
            {other.length > 0 ? (
              <OtherProviders
                groups={other}
                connect={(providerId) => {
                  setConnection({ providerId });
                }}
              />
            ) : null}
            {groups.length === 0 ? (
              <Surface variant="secondary">
                <p className="p-4 text-sm text-muted">
                  Connect an execution node to configure its AI providers.
                </p>
              </Surface>
            ) : null}
            {groups.length > 0 && filtered.length === 0 ? (
              <div className="grid justify-items-start gap-3">
                <output className="text-sm text-muted">
                  No providers or accounts match these filters.
                </output>
                <Button
                  size="sm"
                  variant="secondary"
                  onPress={() => {
                    setQuery("");
                    setSelected(null);
                  }}
                >
                  Clear filters
                </Button>
              </div>
            ) : null}
          </div>
        </>
      ) : (
        <AccountConfiguration
          key={JSON.stringify([catalog.scope, accountId])}
          account={catalog.accounts.find((account) => account.id === accountId)}
          defaults={providers.defaults}
          onReconnect={
            providers.authorization === null
              ? undefined
              : () => {
                  const account = catalog.accounts.find((entry) => entry.id === accountId);
                  if (account !== undefined) {
                    setConnection({ providerId: account.providerId, accountId: account.id });
                  }
                }
          }
        />
      )}
      <ResponsiveDialog
        isOpen={connection !== null}
        onOpenChange={(open) => {
          if (!open) {
            setConnection(null);
          }
        }}
      >
        <ResponsiveDialog.Backdrop>
          <ResponsiveDialog.Container size="md">
            <ResponsiveDialog.Dialog>
              <ResponsiveDialog.CloseTrigger />
              <ResponsiveDialog.Header>
                <ResponsiveDialog.Heading>Connect provider</ResponsiveDialog.Heading>
              </ResponsiveDialog.Header>
              <ResponsiveDialog.Body>
                {connection ? (
                  <ProvidersPanel
                    key={connection.providerId ?? "choose"}
                    providers={providers}
                    initialProviderId={connection.providerId}
                    initialAccountId={connection.accountId}
                    onConfigured={() => {
                      setConnection(null);
                    }}
                  />
                ) : null}
              </ResponsiveDialog.Body>
            </ResponsiveDialog.Dialog>
          </ResponsiveDialog.Container>
        </ResponsiveDialog.Backdrop>
      </ResponsiveDialog>
    </div>
  );
}
