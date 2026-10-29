import { Avatar, Button, ProviderLogo, Separator, Surface } from "@swarmie/ui-web";
import { Plus } from "lucide-react";

import type { ProviderAccount, ProviderChoice } from "./index";

export type ProviderGroup = {
  id: string;
  label: string;
  choice: ProviderChoice | undefined;
  accounts: ProviderAccount[];
};

function Availability({ available }: { available: boolean }) {
  return (
    <span className="inline-flex shrink-0 items-center gap-2 text-xs text-muted">
      <span
        aria-hidden="true"
        className={`size-1.5 rounded-full ${available ? "bg-success" : "bg-muted"}`}
      />
      {available ? "Available" : "Unavailable"}
    </span>
  );
}

/** Provider identity is always visible; narrow placements reflow rows without expanding them. */
export function ProviderAccounts({
  group,
  connect,
  configure,
}: {
  group: ProviderGroup;
  connect: () => void;
  configure: (accountId: string) => void;
}) {
  return (
    <Surface
      variant="secondary"
      aria-label={group.label}
      className="@container min-w-0"
      render={(props) => <section {...props} />}
    >
      <header className="flex flex-wrap items-center justify-between gap-3 p-4">
        <div className="flex min-w-0 items-center gap-3">
          <ProviderLogo providerId={group.id} className="size-6" />
          <div className="flex min-w-0 flex-wrap items-baseline gap-x-3 gap-y-1">
            <h2 className="text-sm font-medium wrap-anywhere">{group.label}</h2>
            <p className="text-xs text-muted">
              {group.accounts.length} {group.accounts.length === 1 ? "account" : "accounts"}
            </p>
          </div>
        </div>
        {group.choice && group.choice.methods.length > 0 ? (
          <Button
            size="sm"
            variant="secondary"
            aria-label={`Add ${group.label} account`}
            onPress={connect}
          >
            <Plus />
            Add account
          </Button>
        ) : null}
      </header>
      <Separator />
      <div
        aria-hidden="true"
        className="hidden grid-cols-[minmax(0,1fr)_5rem_7rem_7rem] items-center gap-4 px-4 py-3 text-xs text-muted @min-[640px]:grid"
      >
        <span>Account</span>
        <span>Models</span>
        <span>Status</span>
        <span />
      </div>
      <ul className="divide-y divide-border">
        {group.accounts.map((account) => (
          <li
            key={account.id}
            className="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-4 p-4 @min-[640px]:grid-cols-[minmax(0,1fr)_5rem_7rem_7rem]"
          >
            <div className="order-1 flex min-w-0 items-center gap-3">
              <Avatar size="sm">
                <Avatar.Fallback>
                  {account.label.trim().slice(0, 2).toLocaleUpperCase() || "AI"}
                </Avatar.Fallback>
              </Avatar>
              <p className="min-w-0 text-sm font-medium wrap-anywhere">{account.label}</p>
            </div>
            <p className="order-3 text-xs text-muted @min-[640px]:order-2">
              {account.models.length} {account.models.length === 1 ? "model" : "models"}
            </p>
            <div className="order-2 @min-[640px]:order-3">
              <Availability available={account.available} />
            </div>
            <Button
              size="sm"
              variant="secondary"
              aria-label={`Configuration for ${account.label}`}
              className="order-4"
              onPress={() => {
                configure(account.id);
              }}
            >
              Configuration
            </Button>
          </li>
        ))}
      </ul>
    </Surface>
  );
}

export function OtherProviders({
  groups,
  connect,
}: {
  groups: ProviderGroup[];
  connect: (providerId: string) => void;
}) {
  return (
    <section aria-label="Other providers" className="grid gap-3">
      <h2 className="text-sm font-medium">Other providers</h2>
      <Surface variant="secondary">
        <ul className="divide-y divide-border">
          {groups.map((group) => (
            <li key={group.id} className="flex flex-wrap items-center justify-between gap-3 p-4">
              <div className="flex min-w-0 items-center gap-3">
                <ProviderLogo providerId={group.id} className="size-6" />
                <div className="grid min-w-0 gap-1">
                  <h3 className="text-sm font-medium wrap-anywhere">{group.label}</h3>
                  <p className="text-xs text-muted">Not connected</p>
                </div>
              </div>
              {group.choice && group.choice.methods.length > 0 ? (
                <Button
                  size="sm"
                  variant="secondary"
                  aria-label={`Connect ${group.label}`}
                  onPress={() => {
                    connect(group.id);
                  }}
                >
                  Connect
                </Button>
              ) : null}
            </li>
          ))}
        </ul>
      </Surface>
    </section>
  );
}
