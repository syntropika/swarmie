import { useSource } from "@swarmie/client-ui";
import type { Conversation, Draft } from "@swarmie/plugin-conversations-browser";
import { providerDisplayName } from "@swarmie/plugin-providers-browser";
import { ModelPicker } from "@swarmie/ui-web";

import { useClient } from "../../client/context";
import { useSelection } from "../../client/use-selection";
import { ModelTrigger } from "./model-trigger";
import { patchDraft } from "./state";

const modelSelection = ({ account, model, effort }: Draft) => ({ account, model, effort });
const sameSelection = (
  a: ReturnType<typeof modelSelection>,
  b: ReturnType<typeof modelSelection>,
) => a.account === b.account && a.model === b.model && a.effort === b.effort;

export function ComposerModel({ chat }: { chat: Conversation }) {
  const { providers, navigate } = useClient();
  const providerState = useSource(providers.state);
  const draft = useSelection(chat.draft, modelSelection, sameSelection);
  const selectedAccount = providerState.accounts.find((account) => account.id === draft.account);
  const selectedModel = selectedAccount?.models.find((model) => model.id === draft.model);
  const accounts = providerState.accounts
    .filter((account) => account.available)
    .map((account) => ({
      ...account,
      providerLabel: providerDisplayName(account.providerId, account.providerLabel),
    }));
  const thinking = selectedModel?.efforts.includes(draft.effort) === true ? draft.effort : "";
  const label = `${selectedModel?.label ?? "Select model"}${thinking ? ` · ${thinking}` : ""}: Select provider, model and thinking`;
  function configure() {
    navigate({ section: "settings", settings: "providers" });
  }
  if (selectedModel === undefined) {
    return (
      <ModelTrigger
        open={false}
        label="Configure providers"
        providerId={selectedAccount?.providerId}
        thinking=""
        onPress={configure}
      />
    );
  }
  return (
    <ModelPicker
      accounts={accounts}
      accountId={draft.account}
      modelId={draft.model}
      effort={draft.effort}
      renderTrigger={({ open }) => (
        <ModelTrigger
          open={open}
          label={label}
          providerId={selectedAccount?.providerId}
          thinking={thinking}
          levels={selectedModel.efforts}
        />
      )}
      onModelSelect={(account, model) => {
        const selection = providers.selectModel({ ...chat.draft.getSnapshot(), account }, model);
        if (selection) {
          patchDraft(chat, selection);
        }
      }}
      onEffortChange={(effort) => {
        const current = chat.draft.getSnapshot();
        const account = providers.state
          .getSnapshot()
          .accounts.find((entry) => entry.id === current.account && entry.available);
        const model = account?.models.find((entry) => entry.id === current.model);
        if (model?.efforts.includes(effort) === true) {
          patchDraft(chat, { effort });
        }
      }}
      onConfigure={configure}
    />
  );
}
