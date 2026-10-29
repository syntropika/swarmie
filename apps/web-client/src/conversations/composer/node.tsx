import { useSource } from "@swarmie/client-ui";
import type { ConversationControlProps } from "@swarmie/plugin-conversations-browser";
import { ChromeButton } from "@swarmie/ui-web";
import { Monitor } from "lucide-react";

import { useClient } from "../../client/context";
import { useSelection } from "../../client/use-selection";
import { iconStyle } from "../../controls";
import { NodeMenu } from "../../node-menu";
import { draftNode } from "./state";
import { useComposerConversation } from "./use-conversation";

export function ComposerNode({ conversationId }: ConversationControlProps) {
  const { connectionState, setConversationNode } = useClient();
  const chat = useComposerConversation(conversationId);
  const nodeId = useSelection(chat.draft, draftNode);
  const { nodes, sessions } = useSource(connectionState);
  const name = nodes.find((node) => node.id === nodeId)?.label ?? "Local";
  const phase = sessions[nodeId]?.phase ?? "unconfigured";
  const label = `${name}: ${phase}. Select execution node`;
  return (
    <NodeMenu
      nodes={nodes.filter((node) => chat.node === null || node.id === chat.node)}
      selected={nodeId}
      selectionLabel={chat.node === null ? "Execution node" : "Conversation node"}
      onSelect={(id) => {
        setConversationNode(chat, id);
      }}
      placement="top end"
    >
      {(open) => (
        <ChromeButton
          size="composerCap"
          aria-label={label}
          title={label}
          selected={open}
          className="max-w-full"
        >
          <Monitor className={iconStyle} />
          <span className="max-w-(--spacing-composer-label) min-w-0 truncate">{name}</span>
        </ChromeButton>
      )}
    </NodeMenu>
  );
}
