import { Pencil, X, Pause, Play, Check } from "lucide-react";
import { useState } from "react";

import { startAction } from "~/web/src/action-boundary";
import { Button } from "~/web/src/components/ui/button";
import { Textarea } from "~/web/src/components/ui/textarea";
import type { Thread, Queued } from "~/web/src/protocol";
import { hasNumber } from "~/web/src/value-presence";
import type { WorkspaceStore } from "~/web/src/workspace";

function QueueItem({
  item,
  thread,
  store,
}: {
  item: Queued;
  thread: Thread;
  store: WorkspaceStore;
}) {
  const [editing, setEditing] = useState(false);
  const [text, setText] = useState(
    item.prompt.content
      .filter((p) => p.type === "text")
      .map((p) => p.data)
      .join("\n"),
  );
  const run = async (action: () => Promise<unknown>) => {
    try {
      await action();
      setEditing(false);
    } catch (error) {
      store.setError(error);
    }
  };
  return (
    <div className="queue-item">
      {editing ? (
        <Textarea
          aria-label="Edit queued message"
          value={text}
          onChange={(e) => {
            setText(e.target.value);
          }}
        />
      ) : (
        <span>{text || "Attached content"}</span>
      )}
      <Button
        variant="ghost"
        size="icon"
        aria-label={editing ? "Save queued message" : "Edit queued message"}
        onClick={() => {
          if (!editing) {
            setEditing(true);
            return;
          }
          startAction(
            run(async () => {
              await store.editQueue(thread.id, item.id, text);
            }),
            store.setError.bind(store),
          );
        }}
      >
        {editing ? <Check size={13} /> : <Pencil size={13} />}
      </Button>
      <Button
        variant="ghost"
        size="icon"
        aria-label="Remove queued message"
        onClick={() => {
          startAction(
            run(async () => {
              await store.editQueue(thread.id, item.id);
            }),
            store.setError.bind(store),
          );
        }}
      >
        <X size={13} />
      </Button>
    </div>
  );
}
export function Queue({ thread, store }: { thread: Thread; store: WorkspaceStore }) {
  if (!hasNumber(thread.queue?.length)) {
    return null;
  }
  return (
    <section className="queue-list" aria-label="Queued messages">
      <div className="queue-heading">
        <span>
          {thread.queue.length} queued follow-up
          {thread.queue.length > 1 ? "s" : ""}
        </span>
        <Button
          variant="ghost"
          size="sm"
          onClick={() => {
            startAction(
              store.pauseQueue(thread.id, !(thread.queuePaused === true)),
              store.setError.bind(store),
            );
          }}
        >
          {thread.queuePaused === true ? <Play size={12} /> : <Pause size={12} />}
          {thread.queuePaused === true ? "Resume queue" : "Pause queue"}
        </Button>
      </div>
      {thread.queue.map((item) => (
        <QueueItem key={item.id} item={item} thread={thread} store={store} />
      ))}
    </section>
  );
}
