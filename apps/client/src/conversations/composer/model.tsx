import { useSource } from "@swarmie/client-ui";
import type { Conversation, Draft } from "@swarmie/plugin-conversations-browser";
import { useState } from "react";

import { useClient } from "../../client/context";
import { useSelection } from "../../client/use-selection";
import { Item, Menu } from "../../controls";
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
          {models.map((model) => (
            <Item
              key={model.id}
              detail={model.id === draft.model ? "✓" : ""}
              onClick={() => {
                const selection = providers.selectModel(chat.draft.getSnapshot(), model.id);
                if (selection) {
                  patchDraft(chat, selection);
                }
                close();
              }}
            >
              <span className="block">{model.label}</span>
              {model.description === undefined ? null : (
                <span className="block text-xs text-muted">{model.description}</span>
              )}
            </Item>
          ))}
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
                onClick={() => {
                  setEffortOpen((value) => !value);
                }}
              >
                Effort
              </Item>
              {effortOpen
                ? selectedModel.efforts.map((effort) => (
                    <Item
                      key={effort}
                      detail={effort === draft.effort ? "✓" : ""}
                      onClick={() => {
                        patchDraft(chat, { effort });
                      }}
                    >
                      {effort}
                    </Item>
                  ))
                : null}
            </>
          ) : null}
        </>
      )}
    </Menu>
  );
}
