import type { Meta, StoryObj } from "@storybook/react-vite";
import { useState } from "react";

import { modelPickerAccounts } from "../../../../../tests/fixtures/ui/model-picker-catalog";
import { ChromeButton } from "../atoms/chrome-button";
import { ProviderLogo } from "../atoms/provider-logo";
import { ModelPicker } from "./model-picker";

const meta = {
  title: "Elements/ModelPicker",
  component: ModelPicker,
  args: {
    accounts: modelPickerAccounts,
    accountId: "personal",
    modelId: "example-1",
    effort: "medium",
    renderTrigger: () => null,
    onModelSelect: () => undefined,
    onEffortChange: () => undefined,
    onConfigure: () => undefined,
  },
  excludeStories: ["ModelPickerExample"],
} satisfies Meta<typeof ModelPicker>;
export default meta;
type Story = StoryObj<typeof meta>;

export function ModelPickerExample({ empty = false }: { empty?: boolean }) {
  const [selection, setSelection] = useState({
    account: "personal",
    model: "example-1",
    effort: "medium",
  });
  const [configured, setConfigured] = useState(false);
  const accounts = empty ? [] : modelPickerAccounts;
  const account = accounts.find((entry) => entry.id === selection.account);
  const model = account?.models.find((entry) => entry.id === selection.model);
  return (
    <div className="grid gap-4">
      <ModelPicker
        accounts={accounts}
        accountId={selection.account}
        modelId={selection.model}
        effort={selection.effort}
        renderTrigger={() => (
          <ChromeButton size="action" aria-label="Select model">
            <ProviderLogo providerId={account?.providerId} />
            {model?.label ?? "Select model"}
          </ChromeButton>
        )}
        onModelSelect={(accountId, modelId) => {
          const chosen = accounts
            .find((entry) => entry.id === accountId)
            ?.models.find((entry) => entry.id === modelId);
          setSelection({
            account: accountId,
            model: modelId,
            effort:
              chosen?.efforts.includes(selection.effort) === true
                ? selection.effort
                : (chosen?.efforts[0] ?? ""),
          });
        }}
        onEffortChange={(effort) => {
          setSelection((current) => ({ ...current, effort }));
        }}
        onConfigure={() => {
          setConfigured(true);
        }}
      />
      <output className="text-caption text-muted">
        {configured
          ? "Settings / Providers"
          : `Selected: ${account?.providerLabel ?? "None"} / ${account?.label ?? "None"} / ${model?.label ?? "None"} / ${selection.effort}`}
      </output>
    </div>
  );
}
export const Default: Story = { render: () => <ModelPickerExample /> };
export const Empty: Story = { render: () => <ModelPickerExample empty /> };
