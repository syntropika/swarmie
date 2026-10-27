import { useSource } from "@swarmie/client-ui";
import type { ConversationControlProps } from "@swarmie/plugin-conversations-browser";
import { Check, Monitor } from "lucide-react";

import { useClient } from "../../client/context";
import { useSelection } from "../../client/use-selection";
import { Caption, Item, Menu } from "../../controls";
import { draftNode } from "./state";
import { useComposerConversation } from "./use-conversation";

export function ComposerNode({ conversationId }: ConversationControlProps) {
  const { connectionState, setConversationNode } = useClient();
  const chat = useComposerConversation(conversationId);
  const nodeId = useSelection(chat.draft, draftNode);
  const { nodes } = useSource(connectionState);
  return (
    <Menu
      label="Select execution node"
      icon={Monitor}
      text={nodes.find((node) => node.id === nodeId)?.label ?? "Local"}
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
                icon={node.id === nodeId ? Check : Monitor}
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
