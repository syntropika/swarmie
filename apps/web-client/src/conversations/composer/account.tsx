import { useSource } from "@swarmie/client-ui";
import type { ConversationControlProps } from "@swarmie/plugin-conversations-browser";
import { Puzzle } from "lucide-react";

import { useClient } from "../../client/context";
import { useSelection } from "../../client/use-selection";
import { Caption, ChoiceList, Item, Menu } from "../../controls";
import { draftAccount, patchDraft } from "./state";
import { useComposerConversation } from "./use-conversation";

export function ComposerAccount({ conversationId }: ConversationControlProps) {
  const { providers, navigate } = useClient();
  const chat = useComposerConversation(conversationId);
  const accountId = useSelection(chat.draft, draftAccount);
  const providerState = useSource(providers.state);
  const { account } = providers.resolve(chat.draft.getSnapshot());
  return (
    <Menu
      label="Select provider account"
      text={account ? `${account.providerLabel} · ${account.label}` : "Select provider"}
      size="composerCap"
      placement="top start"
    >
      {(close) => (
        <>
          <Caption>Provider accounts</Caption>
          {providerState.accounts.length > 0 ? (
            <ChoiceList
              label="Provider accounts"
              value={accountId}
              items={providerState.accounts.map((entry) => ({
                id: entry.id,
                label: `${entry.providerLabel} · ${entry.label}`,
              }))}
              onAction={(id) => {
                const selection = providers.selectAccount(id, chat.draft.getSnapshot());
                if (selection) {
                  patchDraft(chat, selection);
                }
                close();
              }}
            />
          ) : null}
          {providerState.accounts.length === 0 ? (
            <p className="px-2 py-2 text-xs text-muted">Connect a provider to see its models.</p>
          ) : null}
          <Item
            icon={Puzzle}
            onClick={() => {
              navigate({ section: "settings", settings: "providers" });
              close();
            }}
          >
            Configure providers
          </Item>
        </>
      )}
    </Menu>
  );
}
