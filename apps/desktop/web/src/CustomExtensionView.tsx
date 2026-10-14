import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import { Button } from "@/components/ui/button";
import type { ExtensionInfo, ExtensionView, Thread } from "./protocol";
import type { WorkspaceStore } from "./workspace";
import { useContributions } from "./ContributionHost";
import { extensionDocument } from "./extensionFrame";
export function CustomExtensionView({
  store,
  thread,
  extension,
  view,
  context,
}: {
  store: WorkspaceStore;
  thread: Thread;
  extension: ExtensionInfo;
  view: ExtensionView;
  context?: import("./contributions").ContributionContext;
}) {
  const host = useContributions();
  useSyncExternalStore(host?.registry.subscribe ?? (() => () => {}), host?.registry.getSnapshot ?? (() => 0));
  const lifetime = host?.registry.getLifetime(extension.id);
  const frame = useRef<HTMLIFrameElement>(null);
  const [source, setSource] = useState("");
  const [revision, setRevision] = useState(0);
  const [error, setError] = useState("");
  useEffect(() => {
    let disposed = false;
    const channel = crypto.randomUUID();
    const api = extension.ui && lifetime ? host?.registry.api(extension.id, context) : undefined;
    const off = api ? ["context.changed", "composer.changed", "surface.opened", "surface.closed"].map((type) => api.on(type as import("./contributions").ContributionEvent["type"], (event) => {
      if (!disposed && !api.signal.aborted) frame.current?.contentWindow?.postMessage({ channel, event }, "*");
    })) : [];
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
      const { id, command, operation = "runtime.invoke", arguments: args } = event.data;
      if (!Number.isSafeInteger(id) || id < 1) return;
      const reply = (body: Record<string, unknown>) => {
        if (!disposed)
          frame.current?.contentWindow?.postMessage(
            { channel, id, ...body },
            "*",
          );
      };
      if (typeof operation !== "string") return;
      if (operation.startsWith("client.")) {
        try {
          if (!api || api.signal.aborted) throw new Error("This view has no active v1 client API.");
          switch (operation) {
            case "client.context": reply({ result: api.context }); break;
            case "client.insertText": api.insertText(event.data.text, { separateWords: event.data.options?.separateWords === true }); reply({ result: null }); break;
            case "client.openScreen": api.openScreen(event.data.surfaceId); reply({ result: null }); break;
            case "client.openPanel": api.openPanel(event.data.surfaceId); reply({ result: null }); break;
            default: throw new Error("Unknown client operation.");
          }
        } catch (error) { reply({ error: String(error) }); }
        return;
      }
      if (operation !== "runtime.invoke" || (api && api.signal.aborted) || !extension.commands?.some((c) => c.name === command)) {
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
      off.forEach((unsubscribe) => unsubscribe()); api?.dispose();
    };
  }, [
    extension,
    lifetime,
    host?.registry,
    thread.id,
    context?.messageId,
    context?.projectId,
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
