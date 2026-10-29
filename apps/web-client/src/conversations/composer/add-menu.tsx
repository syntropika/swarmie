import { useSource } from "@swarmie/client-ui";
import type { Conversation } from "@swarmie/plugin-conversations-browser";
import { Plus, Puzzle } from "lucide-react";

import { useClient } from "../../client/context";
import { Caption, Item, Menu } from "../../controls";
import type { ComposerActions } from "./actions";

export function ComposerAddMenu({
  chat,
  actions,
}: {
  chat: Conversation;
  actions: ComposerActions;
}) {
  const { conversationsUi, navigate } = useClient();
  const attachments = useSource(conversationsUi.composer.attachmentActions);
  const commands = useSource(conversationsUi.composer.actions);
  useSource(chat.draft);
  return (
    <Menu label="Add to conversation" icon={Plus} size="composerAdd" placement="top start">
      {(close) => (
        <>
          {[...new Set(attachments.map((action) => action.section))].map((section) => (
            <div key={section}>
              <Caption>{section}</Caption>
              {attachments
                .filter((action) => action.section === section)
                .map((action) => (
                  <Item
                    key={action.id}
                    icon={action.icon}
                    onClick={() => {
                      actions.executeAction(action);
                      close();
                    }}
                  >
                    {action.label}
                  </Item>
                ))}
            </div>
          ))}
          {commands.length > 0 ? <Caption>Conversation actions</Caption> : null}
          {commands.map((action) => (
            <Item
              key={action.id}
              icon={action.icon}
              disabled={action.enabled?.(actions.actionContext()) === false}
              onClick={() => {
                actions.executeAction(action);
                close();
              }}
            >
              {action.label}
            </Item>
          ))}
          <Caption>Plugins</Caption>
          <Item
            icon={Puzzle}
            onClick={() => {
              navigate({ section: "settings", settings: "plugins" });
              close();
            }}
          >
            Manage plugins
          </Item>
        </>
      )}
    </Menu>
  );
}
