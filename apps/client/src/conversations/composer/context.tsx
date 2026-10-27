import { useSource } from "@swarmie/client-ui";
import type { ConversationControlProps } from "@swarmie/plugin-conversations-browser";
import { Check, Monitor, Puzzle } from "lucide-react";

import { useClient } from "../../client/context";
import { Caption, Item, Menu } from "../../controls";

function useComposerConversation(id: string) {
  const { roster } = useClient();
  const chats = useSource(roster);
  const chat = chats.find((entry) => entry.id === id);
  if (chat === undefined) {
    throw new Error("The composer conversation is unavailable.");
  }
  return chat;
}
export function ComposerAccount({ conversationId }: ConversationControlProps) {
  const { providers, navigate } = useClient();
  const chat = useComposerConversation(conversationId);
  const draft = useSource(chat.draft);
  const providerState = useSource(providers.state);
  const { account } = providers.resolve(draft);
  const patch = (change: Partial<typeof draft>) => {
    chat.draft.update((current) => ({ ...current, ...change }));
  };
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
          {providerState.accounts.map((entry) => (
            <Item
              key={entry.id}
              icon={entry.id === draft.account ? Check : undefined}
              onClick={() => {
                const selection = providers.selectAccount(entry.id, draft);
                if (selection) {
                  patch(selection);
                }
                close();
              }}
            >
              {entry.providerLabel} · {entry.label}
            </Item>
          ))}
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
export function ComposerNode({ conversationId }: ConversationControlProps) {
  const { connectionState, setConversationNode } = useClient();
  const chat = useComposerConversation(conversationId);
  const draft = useSource(chat.draft);
  const { nodes } = useSource(connectionState);
  return (
    <Menu
      label="Select execution node"
      icon={Monitor}
      text={nodes.find((node) => node.id === draft.node)?.label ?? "Local"}
      size="composerCap"
      placement="top end"
    >
      {(close) => (
        <>
          <Caption>{chat.node === null ? "Execution node" : "Conversation node"}</Caption>
          {nodes
            .filter((node) => chat.node === null || node.id === chat.node)
            .map((node) => (
              <Item
                key={node.id}
                icon={node.id === draft.node ? Check : Monitor}
                onClick={() => {
                  setConversationNode(chat, node.id);
                  close();
                }}
              >
                {node.label}
              </Item>
            ))}
        </>
      )}
    </Menu>
  );
}
