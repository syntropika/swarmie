import { useSource } from "@swarmie/client-ui";
import type { ComposerAction, Conversation } from "@swarmie/plugin-conversations-browser";

import { useClient } from "../../client/context";
import { IconButton } from "../../controls";
import type { ComposerActions } from "./actions";

export function ComposerPluginActions({
  chat,
  slot,
  actions,
}: {
  chat: Conversation;
  slot: ComposerAction["slot"];
  actions: ComposerActions;
}) {
  const { conversationsUi } = useClient();
  const contributions = useSource(conversationsUi.composer.actions);
  useSource(chat.draft);
  return contributions
    .filter((action) => action.slot === slot)
    .map((action) => (
      <IconButton
        key={action.id}
        icon={action.icon}
        size="composer"
        label={action.label}
        disabled={action.enabled?.(actions.actionContext()) === false}
        onPress={() => {
          actions.executeAction(action);
        }}
      />
    ));
}
