import { ChevronRight, CircleUserRound, Plus } from "lucide-react";
import type { ReactNode } from "react";
import { useState } from "react";

import { ChromeButton } from "../atoms/chrome-button";
import { ListBox } from "../atoms/list-box";
import { Popover } from "../atoms/popover";
import { ProviderLogo } from "../atoms/provider-logo";
import { ReasoningEffort } from "../atoms/reasoning-effort";
import { ActionMenuDialog } from "../molecules/action-menu-dialog";
import { usePhoneLayout } from "../use-phone-layout";

export type ModelPickerAccount = {
  id: string;
  label: string;
  providerId: string;
  providerLabel: string;
  models: readonly {
    id: string;
    label: string;
    description?: string;
    efforts: readonly string[];
  }[];
};

export type ModelPickerProps = {
  accounts: readonly ModelPickerAccount[];
  accountId: string | null;
  modelId: string;
  effort: string;
  renderTrigger: (state: { open: boolean }) => ReactNode;
  onModelSelect: (accountId: string, modelId: string) => void;
  onEffortChange: (effort: string) => void;
  onConfigure: () => void;
};

function effortLabel(value: string) {
  const normalized = value.toLowerCase();
  if (normalized === "xhigh") {
    return "Extra high";
  }
  return value.charAt(0).toUpperCase() + value.slice(1);
}

function AccountPicker({
  account,
  accounts,
  onChoose,
  onConfigure,
}: {
  account: ModelPickerAccount;
  accounts: readonly ModelPickerAccount[];
  onChoose: (id: string) => void;
  onConfigure: () => void;
}) {
  const [open, setOpen] = useState(false);
  const phone = usePhoneLayout();
  return (
    <Popover isOpen={open} onOpenChange={setOpen}>
      <ChromeButton
        size="project"
        data-model-account-trigger=""
        aria-label={`${account.label}: Select account`}
        title={account.label}
      >
        <CircleUserRound aria-hidden="true" />
        <span>{account.label}</span>
        <ChevronRight aria-hidden="true" />
      </ChromeButton>
      <Popover.Content
        placement={phone ? "bottom end" : "right top"}
        className="z-50 w-(--spacing-menu-width) max-w-[calc(100vw-24px)]"
      >
        <ActionMenuDialog aria-label={`${account.providerLabel} accounts`}>
          <ChromeButton
            size="navigation"
            onPress={() => {
              setOpen(false);
              onConfigure();
            }}
          >
            <Plus aria-hidden="true" /> Add account
          </ChromeButton>
          <div className="my-1 border-t border-border-subtle" />
          <ListBox
            aria-label="Account"
            selectionMode="single"
            disallowEmptySelection
            selectedKeys={[account.id]}
          >
            {accounts.map((entry) => (
              <ListBox.Item
                key={entry.id}
                id={entry.id}
                textValue={entry.label}
                onPress={() => {
                  setOpen(false);
                  onChoose(entry.id);
                }}
              >
                <CircleUserRound aria-hidden="true" />
                <span className="min-w-0 flex-1 truncate">{entry.label}</span>
                <ListBox.ItemIndicator />
              </ListBox.Item>
            ))}
          </ListBox>
        </ActionMenuDialog>
      </Popover.Content>
    </Popover>
  );
}

function PickerContent({
  accounts,
  accountId,
  modelId,
  effort,
  onModelSelect,
  onEffortChange,
  onConfigure,
}: Omit<ModelPickerProps, "renderTrigger">) {
  const [browsing, setBrowsing] = useState(accountId);
  const account =
    accounts.find((entry) => entry.id === browsing) ??
    accounts.find((entry) => entry.id === accountId) ??
    accounts[0];
  const providerAccounts = accounts.filter((entry) => entry.providerId === account?.providerId);
  const providers = accounts.filter(
    (entry, index) =>
      accounts.findIndex((other) => other.providerId === entry.providerId) === index,
  );
  const selectedModel =
    account?.id === accountId ? account.models.find((entry) => entry.id === modelId) : undefined;
  return (
    <div data-slot="model-picker">
      <fieldset data-slot="model-provider-rail" aria-label="Providers">
        <ChromeButton
          size="application"
          aria-label="Add provider"
          title="Add provider"
          onPress={onConfigure}
        >
          <Plus aria-hidden="true" />
        </ChromeButton>
        {providers.map((provider) => (
          <ChromeButton
            key={provider.providerId}
            size="application"
            aria-label={`${provider.providerLabel} models`}
            title={provider.providerLabel}
            aria-pressed={account?.providerId === provider.providerId}
            onPress={() => {
              setBrowsing(provider.id);
            }}
          >
            <ProviderLogo providerId={provider.providerId} />
          </ChromeButton>
        ))}
      </fieldset>
      <div data-slot="model-picker-main">
        {account ? (
          <>
            <div data-slot="model-picker-account">
              <AccountPicker
                account={account}
                accounts={providerAccounts}
                onChoose={setBrowsing}
                onConfigure={onConfigure}
              />
            </div>
            <p data-slot="model-picker-caption">Select model</p>
            <div data-slot="model-picker-models">
              <ListBox
                aria-label={`${account.providerLabel} models`}
                selectionMode="single"
                disallowEmptySelection
                selectedKeys={account.id === accountId ? [modelId] : []}
              >
                {account.models.map((model) => (
                  <ListBox.Item
                    key={model.id}
                    id={model.id}
                    textValue={model.label}
                    onPress={() => {
                      onModelSelect(account.id, model.id);
                    }}
                  >
                    <span className="min-w-0 flex-1 truncate" title={model.description}>
                      {model.label}
                    </span>
                    <ListBox.ItemIndicator />
                  </ListBox.Item>
                ))}
              </ListBox>
              {account.models.length === 0 ? (
                <p className="p-2 text-sm text-muted">No models available for this account.</p>
              ) : null}
            </div>
            {selectedModel && selectedModel.efforts.length > 0 ? (
              <div data-slot="model-picker-effort">
                <ReasoningEffort
                  levels={selectedModel.efforts.map((id) => ({ id, label: effortLabel(id) }))}
                  value={effort}
                  onChange={onEffortChange}
                  model={selectedModel.label}
                />
              </div>
            ) : null}
          </>
        ) : (
          <p className="p-3 text-sm text-muted">Connect a provider to see its models.</p>
        )}
      </div>
    </div>
  );
}

/** Browsing an account does not change execution selection. Choosing a model commits it. */
export function ModelPicker({
  renderTrigger,
  onModelSelect,
  onConfigure,
  ...props
}: ModelPickerProps) {
  const [open, setOpen] = useState(false);
  return (
    <Popover isOpen={open} onOpenChange={setOpen}>
      {renderTrigger({ open })}
      <Popover.Content
        placement="top end"
        className="z-50 w-(--spacing-menu-model) max-w-[calc(100vw-24px)]"
      >
        <ActionMenuDialog aria-label="Select provider, model and thinking">
          <PickerContent
            {...props}
            onModelSelect={(account, model) => {
              setOpen(false);
              onModelSelect(account, model);
            }}
            onConfigure={() => {
              setOpen(false);
              onConfigure();
            }}
          />
        </ActionMenuDialog>
      </Popover.Content>
    </Popover>
  );
}
