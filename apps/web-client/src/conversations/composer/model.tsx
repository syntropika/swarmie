import { useSource } from "@swarmie/client-ui";
import type { Conversation, Draft } from "@swarmie/plugin-conversations-browser";
import { useState } from "react";

import { useClient } from "../../client/context";
import { useSelection } from "../../client/use-selection";
import { ChoiceList, Item, Menu } from "../../controls";
import { patchDraft } from "./state";

const modelSelection = ({ account, model, effort }: Draft) => ({ account, model, effort });
const sameSelection = (
  a: ReturnType<typeof modelSelection>,
  b: ReturnType<typeof modelSelection>,
) => a.account === b.account && a.model === b.model && a.effort === b.effort;

export function ComposerModel({ chat }: { chat: Conversation }) {
  const { providers } = useClient();
  const providerState = useSource(providers.state);
  const draft = useSelection(chat.draft, modelSelection, sameSelection);
  const models =
    providerState.accounts.find((account) => account.id === draft.account)?.models ?? [];
  const [effortOpen, setEffortOpen] = useState(false);
  const selectedModel = models.find((model) => model.id === draft.model);
  return (
    <Menu
      label="Select model and effort"
      text={selectedModel?.label ?? "Select model"}
      size="composerCap"
      placement="top end"
    >
      {(close) => (
        <>
          {models.length > 0 ? (
            <ChoiceList
              label="Model"
              value={draft.model}
              items={models.map((model) => ({
                id: model.id,
                label: model.label,
                description: model.description,
              }))}
              onAction={(id) => {
                const selection = providers.selectModel(chat.draft.getSnapshot(), id);
                if (selection) {
                  patchDraft(chat, selection);
                }
                close();
              }}
            />
          ) : null}
          {models.length === 0 ? (
            <p className="px-2 py-3 text-xs text-muted">
              No models are available for this account.
            </p>
          ) : null}
          {selectedModel !== undefined && selectedModel.efforts.length > 0 ? (
            <>
              <div className="my-1 border-t border-border" />
              <Item
                detail={draft.effort}
                expanded={effortOpen}
                onClick={() => {
                  setEffortOpen((value) => !value);
                }}
              >
                Effort
              </Item>
              {effortOpen ? (
                <ChoiceList
                  label="Effort"
                  value={draft.effort}
                  items={selectedModel.efforts.map((effort) => ({ id: effort, label: effort }))}
                  onAction={(effort) => {
                    patchDraft(chat, { effort });
                  }}
                />
              ) : null}
            </>
          ) : null}
        </>
      )}
    </Menu>
  );
}
