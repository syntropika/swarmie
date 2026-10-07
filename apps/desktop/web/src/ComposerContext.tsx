import { Dialog } from "@/components/ui/dialog";
import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import type { Thread } from "./protocol";
import type { WorkspaceStore } from "./workspace";
import { fileLocation, mentionAt } from "./context";
import type { ContextItem } from "./context";

export function ComposerContext({
  thread,
  store,
  caret,
  setText,
}: {
  thread: Thread;
  store: WorkspaceStore;
  caret: number;
  setText: (text: string) => void;
}) {
  const text = thread.draft ?? "";
  const mention = mentionAt(text, caret);
  const location = mention ? fileLocation(mention.query) : undefined;
  const [files, setFiles] = useState<string[]>([]);
  const [preview, setPreview] = useState<ContextItem>();
  useEffect(() => {
    let cancelled = false;
    if (!mention) {
      setFiles([]);
      return;
    }
    const timer = setTimeout(
      () =>
        void store
          .findFiles(location?.path ?? mention.query)
          .then((paths) => {
            if (!cancelled) setFiles(paths.slice(0, 8));
          })
          .catch((e) => {
            if (!cancelled) store.setError(e);
          }),
      150,
    );
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [mention?.query, store]);
  return (
    <>
      {preview && <Dialog className="context-dialog" label="Selected message context" onClose={() => setPreview(undefined)}>
        <h3>{preview?.label}</h3>
        <pre className="action-output">{preview?.text}</pre>
        <Button
          variant="ghost"
          className="small-button"
          onClick={() => setPreview(undefined)}
        >
          Close
        </Button>
      </Dialog>}
      {!!thread.contextItems?.length && (
        <div className="context-chips">
          {thread.contextItems.map((item) => (
            <div className="context-chip" key={item.id}>
              <Button
                variant="ghost"
                className="small-button"
                title={item.text.slice(0, 300)}
                onClick={() => {
                  if (item.kind === "resource" || !item.path) setPreview(item);
                  else if (item.path)
                    try {
                      store.openFile({
                        path: item.path,
                        startLine: item.startLine,
                        endLine: item.endLine,
                      });
                    } catch (error) {
                      store.setError(error);
                    }
                }}
              >
                {item.label}
              </Button>
              <Button
                variant="ghost"
                className="small-button"
                aria-label={`Remove context ${item.label}`}
                onClick={() =>
                  store.updateThread(thread.id, {
                    contextItems: thread.contextItems?.filter(
                      (c) => c.id !== item.id,
                    ),
                  })
                }
              >
                Remove
              </Button>
            </div>
          ))}
        </div>
      )}
      {mention && files.length > 0 && (
        <div className="composer-suggestions" aria-label="File suggestions">
          {files.map((path) => (
            <Button
              key={path}
              variant="ghost"
              className="command-result"
              onClick={() => {
                void store
                  .addFileContext(thread.id, {
                    path,
                    startLine: location?.startLine,
                    endLine: location?.endLine,
                  })
                  .then(() => {
                    const current =
                      store
                        .getSnapshot()
                        .threads.find((t) => t.id === thread.id)?.draft ?? text;
                    if (
                      current.slice(mention.start, mention.end) !==
                      text.slice(mention.start, mention.end)
                    )
                      return;
                    const next =
                      current.slice(0, mention.start) +
                      `@${path}${location?.startLine ? `:${location.startLine}-${location.endLine ?? location.startLine}` : ""} ` +
                      current.slice(mention.end);
                    store.updateThread(thread.id, { draft: next });
                    setText(next);
                  })
                  .catch((e) => store.setError(e));
              }}
            >
              {path}
            </Button>
          ))}
        </div>
      )}
    </>
  );
}
