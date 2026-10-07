import { useEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import type { ExtensionInfo, ExtensionView, Thread } from "./protocol";
import type { WorkspaceStore } from "./workspace";
import { extensionDocument } from "./extensionFrame";
export function CustomExtensionView({
  store,
  thread,
  extension,
  view,
}: {
  store: WorkspaceStore;
  thread: Thread;
  extension: ExtensionInfo;
  view: ExtensionView;
}) {
  const frame = useRef<HTMLIFrameElement>(null);
  const [source, setSource] = useState("");
  const [revision, setRevision] = useState(0);
  const [error, setError] = useState("");
  useEffect(() => {
    let disposed = false;
    const channel = crypto.randomUUID();
    setSource("");
    setError("");
    void store
      .invokeExtension<string | { html: string }>(extension.id, view.command, {
        projectId: thread.worktreeId,
        sessionId: thread.sessionId,
      })
      .then((result) => {
        if (!disposed)
          setSource(
            extensionDocument(
              typeof result === "string" ? result : result.html,
              channel,
            ),
          );
      })
      .catch((e) => {
        if (!disposed) setError(String(e));
      });
    const handle = async (event: MessageEvent) => {
      if (
        disposed ||
        !frame.current ||
        event.source !== frame.current.contentWindow ||
        event.data?.channel !== channel
      )
        return;
      const { id, command, arguments: args } = event.data;
      if (!Number.isSafeInteger(id) || id < 1) return;
      const reply = (body: Record<string, unknown>) => {
        if (!disposed)
          frame.current?.contentWindow?.postMessage(
            { channel, id, ...body },
            "*",
          );
      };
      if (!extension.commands?.some((c) => c.name === command)) {
        reply({ error: "This command is not advertised by the extension." });
        return;
      }
      try {
        if (
          !args ||
          typeof args !== "object" ||
          Array.isArray(args) ||
          new TextEncoder().encode(JSON.stringify(args)).length > 1024 * 1024
        )
          throw new Error("Use command arguments under 1 MB.");
        reply({
          result: await store.invokeExtension(extension.id, command, args),
        });
      } catch (e) {
        reply({ error: String(e) });
      }
    };
    window.addEventListener("message", handle);
    return () => {
      disposed = true;
      window.removeEventListener("message", handle);
    };
  }, [
    extension.id,
    view.command,
    thread.worktreeId,
    thread.sessionId,
    revision,
  ]);
  return (
    <div className="custom-extension">
      <div className="review-toolbar">
        <span>{view.title}</span>
        <Button
          variant="ghost"
          className="small-button"
          onClick={() => setRevision((n) => n + 1)}
        >
          Reload view
        </Button>
      </div>
      {error ? (
        <p role="alert" className="inline-error">
          {error}
        </p>
      ) : source ? (
        <iframe
          ref={frame}
          title={view.title}
          srcDoc={source}
          sandbox="allow-scripts"
        />
      ) : (
        <p className="panel-note">Loading extension view…</p>
      )}
    </div>
  );
}
