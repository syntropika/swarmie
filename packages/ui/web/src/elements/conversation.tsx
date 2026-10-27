import type { ReactNode } from "react";

import { Button } from "../atoms/button";
import { TextArea } from "../atoms/textarea";
import { MarkdownText } from "./markdown";

export { ImagePreview } from "../../target/assistant-elements/packages/ui/src/components/react/assistant-ui/elements/image";
export { ThinkingIndicator } from "./thinking-indicator";
export { ComposerPanel } from "./composer-panel";
export { ConversationMap, type ConversationMapEntry } from "./conversation-map";
export { MarkdownText } from "./markdown";
export { MessageActions, type MessageActionsProps } from "./message-actions";
export { ModelSelector, type ModelSelectorProps, type ModelOptionView } from "./model-selector";

/** Client presentation only; identifiers and committed history are supplied by the connected runtime. */
export type ConversationMessageView = {
  id: string;
  role: "user" | "assistant";
  content: string;
  supplementary?: ReactNode;
};
export type ConversationThreadProps = {
  messages: readonly ConversationMessageView[];
  draft: string;
  isRunning: boolean;
  onDraftChange: (draft: string) => void;
  onSend: () => void;
  onCancel: () => void;
};
export function ConversationThread({
  messages,
  draft,
  isRunning,
  onDraftChange,
  onSend,
  onCancel,
}: ConversationThreadProps) {
  return (
    <section aria-label="Conversation" className="flex w-full min-w-0 flex-col gap-4">
      <div role="log" aria-label="Messages" className="grid min-w-0 gap-4">
        {messages.map((message) => (
          <article
            key={message.id}
            className="grid min-w-0 gap-2 rounded-panel border border-border p-4"
          >
            <p className="text-caption font-medium text-text-muted">
              {message.role === "user" ? "You" : "Assistant"}
            </p>
            <MarkdownText text={message.content} />
            {message.supplementary}
          </article>
        ))}
      </div>
      <form
        className="grid gap-3"
        onSubmit={(event) => {
          event.preventDefault();
          onSend();
        }}
      >
        <TextArea
          aria-label="Message"
          value={draft}
          onChange={(event) => {
            onDraftChange(event.target.value);
          }}
          placeholder="Write a message"
        />
        <div className="flex gap-2">
          {isRunning ? (
            <Button variant="secondary" onPress={onCancel}>
              Stop
            </Button>
          ) : (
            <Button type="submit" disabled={!draft.trim()}>
              Send
            </Button>
          )}
        </div>
      </form>
    </section>
  );
}
