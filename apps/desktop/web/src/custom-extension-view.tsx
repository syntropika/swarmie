import { useQuery } from "@tanstack/react-query";
import { useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";

import { Button } from "@/components/ui/button";

import { errorMessage, startAction } from "./action-boundary";
import { isRecord } from "./checked-json";
import { useContributions } from "./contribution-context";
import type { ContributionContext, ContributionEvent, ScopedClientApi } from "./contributions";
import { extensionDocument } from "./extension-frame";
import type { ExtensionInfo, ExtensionView, Thread } from "./protocol";
import type { WorkspaceStore } from "./workspace";

type ViewProps = {
  store: WorkspaceStore;
  thread: Thread;
  extension: ExtensionInfo;
  view: ExtensionView;
  context?: ContributionContext;
};
const events: ContributionEvent["type"][] = [
  "context.changed",
  "composer.changed",
  "surface.opened",
  "surface.closed",
];
const noSubscribe = () => () => undefined;
const zero = () => 0;
function html(value: unknown): string {
  if (typeof value === "string") {
    return value;
  }
  if (isRecord(value) && typeof value.html === "string") {
    return value.html;
  }
  throw new TypeError("The extension returned an invalid document.");
}
function validReceiptId(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value > 0;
}
function clientOperation(api: ScopedClientApi | undefined, packet: Record<string, unknown>) {
  if (api === undefined || api.signal.aborted) {
    throw new Error("This view has no active v1 client API.");
  }
  switch (packet.operation) {
    case "client.context": {
      return api.context;
    }
    case "client.insertText": {
      if (typeof packet.text !== "string") {
        throw new TypeError("Use text for the composer.");
      }
      const separateWords = isRecord(packet.options) && packet.options.separateWords === true;
      api.insertText(packet.text, { separateWords });
      return null;
    }
    case "client.openScreen":
    case "client.openPanel": {
      if (typeof packet.surfaceId !== "string") {
        throw new TypeError("Unknown extension surface.");
      }
      if (packet.operation === "client.openScreen") {
        api.openScreen(packet.surfaceId);
      } else {
        api.openPanel(packet.surfaceId);
      }
      return null;
    }
    default: {
      throw new Error("Unknown client operation.");
    }
  }
}
export function CustomExtensionView(props: ViewProps) {
  const [revision, setRevision] = useState(0);
  const { extension, view, thread, context } = props;
  return (
    <section className="custom-extension">
      <div className="review-toolbar">
        <span>{view.title}</span>
        <Button
          variant="ghost"
          size="sm"
          onClick={() => {
            setRevision((value) => value + 1);
          }}
        >
          Reload view
        </Button>
      </div>
      <ExtensionFrame
        key={JSON.stringify([
          extension.id,
          view.command,
          thread.id,
          thread.worktreeId,
          thread.sessionId,
          context,
          revision,
        ])}
        {...props}
        revision={revision}
      />
    </section>
  );
}
function ExtensionFrame({
  store,
  thread,
  extension,
  view,
  context,
  revision,
}: ViewProps & { revision: number }) {
  const host = useContributions();
  useSyncExternalStore(host?.registry.subscribe ?? noSubscribe, host?.registry.getSnapshot ?? zero);
  const registry = host?.registry;
  const lifetime = registry?.getLifetime(extension.id);
  const frame = useRef<HTMLIFrameElement>(null);
  const channel = useMemo(() => crypto.randomUUID(), []);
  const { worktreeId, sessionId } = thread;
  const document = useQuery({
    queryKey: ["extension-document", extension.id, view.command, worktreeId, sessionId, revision],
    retry: false,
    queryFn: async () =>
      await store.invokeExtension(
        extension.id,
        view.command,
        { projectId: worktreeId, sessionId },
        html,
      ),
  });
  useEffect(() => {
    let disposed = false;
    const api =
      extension.ui !== undefined && lifetime !== undefined
        ? registry?.api(extension.id, context)
        : undefined;
    const off =
      api === undefined
        ? []
        : events.map((type) =>
            api.on(type, (event) => {
              if (!disposed && !api.signal.aborted) {
                frame.current?.contentWindow?.postMessage({ channel, event }, "*");
              }
            }),
          );
    const receive = async (event: MessageEvent<unknown>) => {
      const packet = event.data;
      if (
        disposed ||
        frame.current === null ||
        event.source !== frame.current.contentWindow ||
        !isRecord(packet) ||
        packet.channel !== channel
      ) {
        return;
      }
      const { id } = packet;
      if (!validReceiptId(id)) {
        return;
      }
      const reply = (body: Record<string, unknown>) => {
        if (!disposed) {
          frame.current?.contentWindow?.postMessage({ channel, id, ...body }, "*");
        }
      };
      const operation = packet.operation ?? "runtime.invoke";
      if (typeof operation !== "string") {
        return;
      }
      try {
        if (operation.startsWith("client.")) {
          reply({ result: clientOperation(api, packet) });
          return;
        }
        const { command } = packet;
        if (
          operation !== "runtime.invoke" ||
          api?.signal.aborted === true ||
          typeof command !== "string" ||
          extension.commands?.some((entry) => entry.name === command) !== true
        ) {
          reply({ error: "This command is not advertised by the extension." });
          return;
        }
        const args = packet.arguments;
        if (
          !isRecord(args) ||
          new TextEncoder().encode(JSON.stringify(args)).length > 1024 * 1024
        ) {
          throw new TypeError("Use command arguments under 1 MB.");
        }
        reply({ result: await store.invokeExtension(extension.id, command, args) });
      } catch (error) {
        reply({ error: errorMessage(error) });
      }
    };
    const handle = (event: MessageEvent<unknown>) => {
      startAction(receive(event), (failure) => {
        store.setError(failure);
      });
    };
    window.addEventListener("message", handle);
    return () => {
      disposed = true;
      window.removeEventListener("message", handle);
      for (const unsubscribe of off) {
        unsubscribe();
      }
      api?.dispose();
    };
  }, [channel, context, extension, lifetime, registry, store]);
  if (document.error !== null) {
    return <p className="inline-error">{errorMessage(document.error)}</p>;
  }
  if (document.data === undefined) {
    return <p className="panel-note">Loading extension view…</p>;
  }
  return (
    <iframe
      ref={frame}
      title={view.title}
      srcDoc={extensionDocument(document.data, channel)}
      sandbox="allow-scripts"
    />
  );
}
