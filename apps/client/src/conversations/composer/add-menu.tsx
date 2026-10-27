import { useSource } from "@swarmie/client-ui";
import type { Conversation } from "@swarmie/plugin-conversations-browser";
import { Paperclip, Plus, Puzzle } from "lucide-react";
import { useRef } from "react";

import { useClient } from "../../client/context";
import { Caption, Item, Menu } from "../../controls";
import type { ComposerActions } from "./actions";

export function ComposerAddMenu({
  chat,
  executeAction,
}: { chat: Conversation } & Pick<ComposerActions, "executeAction">) {
  const { conversationsUi, navigate } = useClient();
  const attachments = useSource(conversationsUi.composer.attachmentActions);
  const fileInput = useRef<HTMLInputElement>(null);
  return (
    <>
      <Menu label="Add to conversation" icon={Plus} size="composer" placement="top start">
        {(close) => (
          <>
            <Caption>Add</Caption>
            <Item
              icon={Paperclip}
              onClick={() => {
                fileInput.current?.click();
                close();
              }}
            >
              Files and folders
            </Item>
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
                        executeAction(action);
                        close();
                      }}
                    >
                      {action.label}
                    </Item>
                  ))}
              </div>
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
      <input
        ref={fileInput}
        type="file"
        multiple
        aria-label="Attach files"
        className="hidden"
        onChange={(event) => {
          const names = [...(event.target.files ?? [])].map((file) => file.name);
          chat.draft.update((draft) => ({
            ...draft,
            attachments: [...new Set([...draft.attachments, ...names])],
          }));
        }}
      />
    </>
  );
}
