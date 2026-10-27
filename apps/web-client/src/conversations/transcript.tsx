import { useSource } from "@swarmie/client-ui";
import type { Conversation, Message } from "@swarmie/plugin-conversations-browser";
import type { VirtualListHandle, VirtualListPosition } from "@swarmie/ui-web";
import { VirtualList } from "@swarmie/ui-web";
import type { ConversationMapEntry } from "@swarmie/ui-web/elements/conversation";
import { ConversationMap } from "@swarmie/ui-web/elements/conversation";
import { useMemo, useRef, useState } from "react";

import { useClient } from "../client/context";
import { MessageRow } from "./message-row";

const readingPositions = new Map<string, VirtualListPosition>();
const messageKey = (message: Message) => message.id;
export function Transcript({ chat }: { chat: Conversation }) {
  const { conversationError } = useClient();

  const messages = useSource(chat.messages);
  const error = useSource(conversationError);
  const list = useRef<VirtualListHandle>(null);
  const [visible, setVisible] = useState<readonly string[]>([]);
  const entries = useMemo<ConversationMapEntry[]>(
    () =>
      messages
        .filter((message) => message.role !== "tool")
        .map((message) => ({
          id: message.id,
          title: message.role === "user" ? "You" : "Assistant",
          preview: message.text,
          role: message.role === "user" ? "user" : "assistant",
        })),
    [messages],
  );
  return (
    <div className="@container/conversation relative min-h-0 flex-1">
      <VirtualList
        ref={list}
        items={messages}
        getItemKey={messageKey}
        label="Conversation messages"
        estimateSize={180}
        initialPosition={readingPositions.get(chat.id)}
        onPositionChange={(position) => {
          readingPositions.set(chat.id, position);
        }}
        onVisibleKeysChange={setVisible}
        followEnd
        className="h-full [scrollbar-gutter:stable_both-edges]"
        renderItem={(message) => (
          <div className="px-5 pt-6 md:px-6">
            <MessageRow message={message} chat={chat} />
          </div>
        )}
      />
      {entries.length > 1 && visible.length > 0 && visible.length < messages.length ? (
        <div className="absolute inset-y-4 left-1 z-10 hidden items-center @min-[840px]/conversation:flex">
          <ConversationMap
            entries={entries}
            activeId={visible[0]}
            visibleIds={visible}
            side="left"
            onSelect={(id) => list.current?.scrollToKey(id, { focus: false })}
          />
        </div>
      ) : null}
      {error === null ? null : (
        <div
          role="alert"
          className="absolute inset-x-4 bottom-2 mx-auto max-w-[720px] rounded-lg border border-border bg-overlay px-3 py-2 text-xs text-danger"
        >
          {error}
        </div>
      )}
    </div>
  );
}
