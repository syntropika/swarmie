import type { Conversation } from "@swarmie/plugin-conversations-browser";
import { ComposerTextArea } from "@swarmie/ui-web";
import { useLayoutEffect, useRef } from "react";

import { useClient } from "../../client/context";
import { useSelection } from "../../client/use-selection";
import { draftText, isSlashCommand, patchDraft } from "./state";
import type { DeliveryMode } from "./state";

function fitInput(element: HTMLTextAreaElement | null) {
  if (element) {
    element.style.height = "auto";
    element.style.height = `${element.scrollHeight}px`;
  }
}

export function ComposerInput({ chat, mode }: { chat: Conversation; mode: DeliveryMode }) {
  const { canSend, send } = useClient();
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
          if (
            event.key === "Enter" &&
            !event.shiftKey &&
            !event.nativeEvent.isComposing &&
            !slash &&
            canSend(chat)
          ) {
            event.preventDefault();
            send(chat, mode);
          }
        }}
        placeholder="Ask anything…"
      />
    </>
  );
}
