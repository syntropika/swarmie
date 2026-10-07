import { useState } from "react";
import { Pencil, X, Pause, Play, Check } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import type { Thread, Queued } from "./protocol";
import type { WorkspaceStore } from "./workspace";
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
    } catch (e) {
      store.setError(e);
    }
  };
  return (
    <div className="queue-item">
      {editing ? (
        <Textarea
          aria-label="Edit queued message"
          value={text}
          onChange={(e) => setText(e.target.value)}
        />
      ) : (
        <span>{text || "Attached content"}</span>
      )}
      <Button
        variant="ghost"
        size="icon"
        className="icon-button"
        aria-label={editing ? "Save queued message" : "Edit queued message"}
        onClick={() =>
          editing
            ? void run(() => store.editQueue(thread.id, item.id, text))
            : setEditing(true)
        }
      >
        {editing ? <Check size={13} /> : <Pencil size={13} />}
      </Button>
      <Button
        variant="ghost"
        size="icon"
        className="icon-button"
        aria-label="Remove queued message"
        onClick={() => void run(() => store.editQueue(thread.id, item.id))}
      >
        <X size={13} />
      </Button>
    </div>
  );
}
export function Queue({
  thread,
  store,
}: {
  thread: Thread;
  store: WorkspaceStore;
}) {
  if (!thread.queue?.length) return null;
  return (
    <section className="queue-list" aria-label="Queued messages">
      <div className="queue-heading">
        <span>
          {thread.queue.length} queued follow-up
          {thread.queue.length > 1 ? "s" : ""}
        </span>
        <Button
          variant="ghost"
          className="small-button"
          onClick={() =>
            void store
              .pauseQueue(thread.id, !thread.queuePaused)
              .catch((e) => store.setError(e))
          }
        >
          {thread.queuePaused ? <Play size={12} /> : <Pause size={12} />}
          {thread.queuePaused ? "Resume queue" : "Pause queue"}
        </Button>
      </div>
      {thread.queue.map((item) => (
        <QueueItem key={item.id} item={item} thread={thread} store={store} />
      ))}
    </section>
  );
}
