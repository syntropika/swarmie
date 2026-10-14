import { useEffect, useState } from "react";

import { Button } from "@/components/ui/button";
import { Dialog } from "@/components/ui/dialog";

import { startAction } from "./action-boundary";
import { fileLocation, mentionAt } from "./context";
import type { ContextItem } from "./context";
import type { Thread } from "./protocol";
import { hasText, hasNumber } from "./value-presence";
import type { WorkspaceStore } from "./workspace";

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
  const [results, setResults] = useState<{ query: string; files: string[] }>({
    query: "",
    files: [],
  });
  const search = location?.path ?? mention?.query;
  const files = results.query === search ? results.files : [];
  const [preview, setPreview] = useState<ContextItem>();
  useEffect(() => {
    let cancelled = false;
    if (search === undefined) {
      return undefined;
    }
    const find = async () => {
      try {
        const paths = await store.findFiles(search);
        if (!cancelled) {
          setResults({ query: search, files: paths.slice(0, 8) });
        }
      } catch (error) {
        if (!cancelled) {
          store.setError(error);
        }
      }
    };
    const timer = setTimeout(() => {
      startAction(find(), store.setError.bind(store));
    }, 150);

    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [search, store]);
  return (
    <>
      {preview && (
        <Dialog
          variant="context-dialog"
          label="Selected message context"
          onClose={() => {
            setPreview(undefined);
          }}
        >
          <h3>{preview?.label}</h3>
          <pre className="action-output">{preview?.text}</pre>
          <Button
            variant="ghost"
            size="sm"
            onClick={() => {
              setPreview(undefined);
            }}
          >
            Close
          </Button>
        </Dialog>
      )}
      {hasNumber(thread.contextItems?.length) && (
        <div className="context-chips">
          {thread.contextItems.map((item) => (
            <div className="context-chip" key={item.id}>
              <Button
                variant="ghost"
                size="sm"
                title={item.text.slice(0, 300)}
                onClick={() => {
                  if (item.kind === "resource" || !hasText(item.path)) {
                    setPreview(item);
                  } else if (hasText(item.path)) {
                    try {
                      store.openFile({
                        path: item.path,
                        startLine: item.startLine,
                        endLine: item.endLine,
                      });
                    } catch (error) {
                      store.setError(error);
                    }
                  }
                }}
              >
                {item.label}
              </Button>
              <Button
                variant="ghost"
                size="sm"
                aria-label={`Remove context ${item.label}`}
                onClick={() => {
                  store.updateThread(thread.id, {
                    contextItems: thread.contextItems?.filter((c) => c.id !== item.id),
                  });
                }}
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
              className="w-full"
              onClick={() => {
                const add = async () => {
                  await store.addFileContext(thread.id, {
                    path,
                    startLine: location?.startLine,
                    endLine: location?.endLine,
                  });
                  const current =
                    store.getSnapshot().threads.find((t) => t.id === thread.id)?.draft ?? text;
                  if (
                    current.slice(mention.start, mention.end) !==
                    text.slice(mention.start, mention.end)
                  ) {
                    return;
                  }
                  const next = `${current.slice(0, mention.start)}@${path}${hasNumber(location?.startLine) ? `:${location.startLine}-${location.endLine ?? location.startLine}` : ""} ${current.slice(mention.end)}`;
                  store.updateThread(thread.id, { draft: next });
                  setText(next);
                };
                startAction(add(), store.setError.bind(store));
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
