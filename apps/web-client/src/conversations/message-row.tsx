import { useSource } from "@swarmie/client-ui";
import type { Conversation, Message, MessageAction } from "@swarmie/plugin-conversations-browser";
import { MarkdownText } from "@swarmie/ui-web/elements/conversation";
import { Check, ChevronDown, Copy, FileText, MoreHorizontal, Share } from "lucide-react";
import { useEffect, useMemo, useState } from "react";

import { useClient } from "../client/context";
import { IconButton, copyText } from "../controls";

export function MessageRow({ message, chat }: { message: Message; chat: Conversation }) {
  const { conversationState, conversationsUi, conversationNode } = useClient();

  const [toolOpen, setToolOpen] = useState(false);
  const actions = useSource(conversationsUi.messages.actions);
  const renderers = useSource(conversationsUi.messages.renderers);
  const scope = useMemo(() => new AbortController(), []);
  useEffect(
    () => () => {
      scope.abort();
    },
    [scope],
  );
  const Renderer = renderers.find((renderer) => renderer.matches(message))?.component;
  function failed(error: unknown) {
    conversationState.update((current) => ({
      ...current,
      error: error instanceof Error ? error.message : String(error),
    }));
  }
  async function runAction(action: MessageAction) {
    await action.execute({
      message,
      conversationId: chat.id,
      nodeId: conversationNode(chat),
      signal: scope.signal,
      insertText: (text) => {
        if (!scope.signal.aborted) {
          chat.draft.update((current) => ({ ...current, text: current.text + text }));
        }
      },
    });
  }
  function executeAction(action: MessageAction) {
    runAction(action).catch(failed);
  }
  return (
    <article
      aria-label={`${message.role === "user" ? "You" : "Assistant"} message`}
      className={`group/message mx-auto mb-6 flex w-full max-w-[720px] min-w-0 flex-col ${message.role === "user" ? "items-end" : "items-start"}`}
    >
      {message.tool ? (
        <div className="mb-4 max-w-full">
          <button
            type="button"
            aria-expanded={toolOpen}
            onClick={() => {
              setToolOpen((value) => !value);
            }}
            className="flex items-center gap-2 text-xs text-muted hover:text-foreground"
          >
            {message.tool === "working" ? (
              <span className="size-3 animate-pulse rounded-full border border-brand-accent" />
            ) : (
              <Check className="size-3 text-brand-accent" />
            )}
            <span>Tool call</span>
            <ChevronDown className="size-3" />
          </button>
          {toolOpen ? (
            <div className="mt-2 max-w-full rounded-lg border border-border p-3 text-xs">
              <MarkdownText text={message.text} />
            </div>
          ) : null}
        </div>
      ) : null}
      <div
        className={`min-w-0 text-conversation break-words ${message.role === "user" ? "max-w-[85%] rounded-xl bg-row-selected px-4 py-2.5" : "w-full"}`}
      >
        {(() => {
          if (Renderer) {
            return <Renderer message={message} />;
          }
          if (message.tool) {
            return null;
          }
          return <MarkdownText text={message.text} />;
        })()}
      </div>
      {message.interrupted === true ? (
        <span className="mt-2 text-xs text-muted">Response interrupted</span>
      ) : null}
      <div className="mt-1 flex min-h-(--spacing-chrome-tab-close) flex-wrap items-center gap-0.5 opacity-0 group-focus-within/message:opacity-100 group-hover/message:opacity-100 [@media(hover:none)]:opacity-100">
        <IconButton
          icon={Copy}
          size="tabClose"
          label="Copy message"
          onPress={() => {
            copyText(message.text);
          }}
        />
        <IconButton
          icon={Share}
          size="tabClose"
          label="Share message"
          onPress={() => {
            if (typeof navigator.share === "function") {
              navigator.share({ text: message.text }).catch(() => undefined);
            } else {
              copyText(message.text);
            }
          }}
        />
        {message.role === "user" ? (
          <IconButton
            icon={FileText}
            size="tabClose"
            label="Edit in composer"
            onPress={() => {
              chat.draft.update((current) => ({ ...current, text: message.text }));
            }}
          />
        ) : (
          <IconButton
            icon={MoreHorizontal}
            size="tabClose"
            label="Copy message details"
            onPress={() => {
              copyText(message.text);
            }}
          />
        )}
        {actions
          .filter((action) => !action.roles || action.roles.includes(message.role))
          .map((action) => (
            <IconButton
              key={action.id}
              icon={action.icon}
              size="tabClose"
              label={action.label}
              onPress={() => {
                executeAction(action);
              }}
            />
          ))}
      </div>
    </article>
  );
}
