import { useSource } from "@swarmie/client-ui";
import type { Conversation } from "@swarmie/plugin-conversations-browser";
import type { VirtualListHandle, VirtualListPosition } from "@swarmie/ui-web";
import { VirtualList } from "@swarmie/ui-web";
import type { ConversationMapEntry } from "@swarmie/ui-web/elements/conversation";
import { ConversationMap } from "@swarmie/ui-web/elements/conversation";
import { useMemo, useState } from "react";
import type { RefObject } from "react";

import { useClient } from "../client/context";
import { MessageRow } from "./message-row";
import { transcriptMessageKey, transcriptRowKey, transcriptRows } from "./rows";
import { activityLabel, ConversationStatus } from "./status";
import { ToolResults } from "./tool-results";

const readingPositions = new Map<string, VirtualListPosition>();
export function Transcript({
  chat,
  list,
}: {
  chat: Conversation;
  list: RefObject<VirtualListHandle | null>;
}) {
  const { conversationError, settings } = useClient();

  const messages = useSource(chat.messages);
  const execution = useSource(chat.execution);
  const error = useSource(conversationError);
  const { toolDetails } = useSource(settings);
  const hasActivity = activityLabel(execution) !== "" || error !== null;
  const rows = useMemo(
    () => transcriptRows(messages, toolDetails, hasActivity),
    [messages, toolDetails, hasActivity],
  );
  const [visible, setVisible] = useState<readonly string[]>([]);
  const visibleMessages = visible
    .filter((key) => key.startsWith("message:"))
    .map((key) => key.slice("message:".length));
  const entries = useMemo<ConversationMapEntry[]>(
    () =>
      rows
        .filter((row) => row.kind === "message")
        .map(({ message }) => ({
          id: message.id,
          title: message.role === "user" ? "You" : "Assistant",
          preview: message.text,
          role: message.role === "user" ? "user" : "assistant",
        })),
    [rows],
  );
  return (
    <div className="@container/conversation relative min-h-0 flex-1">
      {(execution.omittedMessages ?? 0) > 0 ? (
        <output className="absolute inset-x-4 top-1 z-10 text-center text-caption text-muted">
          {execution.omittedMessages} earlier messages omitted.
        </output>
      ) : null}
      <VirtualList
        ref={list}
        items={rows}
        getItemKey={transcriptRowKey}
        label="Conversation messages"
        estimateSize={180}
        initialPosition={readingPositions.get(chat.id)}
        onPositionChange={(position) => {
          readingPositions.set(chat.id, position);
        }}
        onVisibleKeysChange={setVisible}
        followEnd
        className="h-full [scrollbar-gutter:stable_both-edges]"
        renderItem={(row) => (
          <div className="px-5 pt-6 md:px-6">
            <div className="mx-auto w-full max-w-[720px] min-w-0">
              {row.kind === "message" ? <MessageRow message={row.message} chat={chat} /> : null}
              {row.kind === "tools" ? <ToolResults messages={row.messages} /> : null}
              {row.kind === "activity" ? <ConversationStatus chat={chat} /> : null}
            </div>
          </div>
        )}
      />
      {entries.length > 1 &&
      visibleMessages.length > 0 &&
      visibleMessages.length < entries.length ? (
        <div className="absolute inset-y-4 left-1 z-10 hidden items-center @min-[840px]/conversation:flex">
          <ConversationMap
            entries={entries}
            activeId={visibleMessages[0]}
            visibleIds={visibleMessages}
            side="left"
            onSelect={(id) => list.current?.scrollToKey(transcriptMessageKey(id), { focus: false })}
          />
        </div>
      ) : null}
    </div>
  );
}
