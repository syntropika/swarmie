import { useSource } from "@swarmie/client-ui";
import type { Conversation } from "@swarmie/plugin-conversations-browser";
import { ComposerTextArea } from "@swarmie/ui-web";
import { useLayoutEffect, useRef } from "react";

import { useClient } from "../../client/context";
import { useSelection } from "../../client/use-selection";
import { executeSlashCommand } from "./actions";
import type { ComposerActions } from "./actions";
import { draftText, isSlashCommand, patchDraft } from "./state";
import type { DeliveryMode } from "./state";

function fitInput(element: HTMLTextAreaElement | null) {
  if (element) {
    element.style.height = "auto";
    element.style.height = `${element.scrollHeight}px`;
  }
}

export function ComposerInput({
  chat,
  mode,
  actions,
}: {
  chat: Conversation;
  mode: DeliveryMode;
  actions: ComposerActions;
}) {
  const { canSend, send, conversationsUi } = useClient();
  const commands = useSource(conversationsUi.composer.slashCommands);
  const text = useSelection(chat.draft, draftText);
  const slash = isSlashCommand(text);
  const input = useRef<HTMLTextAreaElement>(null);
  useLayoutEffect(() => {
    fitInput(input.current);
  });
  useLayoutEffect(() => {
    const element = input.current;
    if (element === null) {
      return undefined;
    }
    let width = element.clientWidth;
    const observer = new ResizeObserver(() => {
      if (element.clientWidth !== width) {
        width = element.clientWidth;
        fitInput(element);
      }
    });
    observer.observe(element);
    return () => {
      observer.disconnect();
    };
  }, []);
  return (
    <>
      <label className="sr-only" htmlFor={`composer-${chat.id}`}>
        Message
      </label>
      <ComposerTextArea
        ref={input}
        id={`composer-${chat.id}`}
        rows={1}
        value={text}
        onChange={(event) => {
          patchDraft(chat, { text: event.target.value });
        }}
        onKeyDown={(event) => {
          if (event.key !== "Enter" || event.shiftKey || event.nativeEvent.isComposing) {
            return;
          }
          if (slash) {
            event.preventDefault();
            const command = commands.find((entry) => text === `/${entry.command}`);
            if (command !== undefined && command.enabled?.(actions.actionContext()) !== false) {
              executeSlashCommand(chat, text, command, actions.executeConfirmed).catch(
                () => undefined,
              );
            }
          } else if (canSend(chat)) {
            event.preventDefault();
            send(chat, mode);
          }
        }}
        placeholder="Ask anything…"
      />
    </>
  );
}
