import type { Conversation } from "@swarmie/plugin-conversations-browser";

import { useClient } from "../../client/context";
import { useSelection } from "../../client/use-selection";
import { draftText, isSlashCommand, patchDraft } from "./state";
import type { DeliveryMode } from "./state";

export function ComposerInput({ chat, mode }: { chat: Conversation; mode: DeliveryMode }) {
  const { canSend, send } = useClient();
  const text = useSelection(chat.draft, draftText);
  const slash = isSlashCommand(text);
  return (
    <>
      <label className="sr-only" htmlFor={`composer-${chat.id}`}>
        Message
      </label>
      <textarea
        id={`composer-${chat.id}`}
        rows={2}
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
        className="block min-h-20 w-full resize-none border-0 bg-transparent px-3 pt-3 pb-2 text-base leading-6 text-foreground placeholder:text-muted @min-[640px]/composer:text-conversation"
      />
    </>
  );
}
