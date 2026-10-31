import { errorMessage } from "@swarmie/client-ui";
import type { Snapshot, UiStore } from "@swarmie/client-ui";
import type {
  Connections,
  ConnectionsState,
  NodeConnection,
} from "@swarmie/plugin-browser/connections";
import type { ConversationsUiApi } from "@swarmie/plugin-conversations-browser";
import { Icon } from "@swarmie/ui-web";
import type { DynamicReference } from "@syntropika/tembra-sdk";

import { compactCommand } from "./types";
import type { Pending, Presentation, Selection } from "./types";
import { compactAlias, contextView, receipt } from "./validation";

async function call(peer: DynamicReference, name: string, args: unknown[] = []): Promise<unknown> {
  const method: unknown = Reflect.get(peer, name);
  if (typeof method !== "function") {
    throw new TypeError("Context is unavailable on this node.");
  }
  return await Reflect.apply(method, undefined, args);
}
function same(left: Selection | null, right: Selection | null) {
  return left?.nodeId === right?.nodeId && left?.conversationId === right?.conversationId;
}
function CompactIcon({ className }: { className?: string }) {
  return <Icon name="memory" className={className} />;
}
function noop() {
  return undefined;
}

/** The local owner observes remote Tembra State. It never owns backend work. */
export function createContextPresentation(
  state: UiStore<Readonly<Presentation>>,
  nodes: Snapshot<Readonly<ConnectionsState>>,
  connections: Connections,
  selection: Snapshot<Selection | null>,
  commands: ConversationsUiApi["composer"],
) {
  let closed = false;
  let epoch = 0;
  let key = "";
  let session: NodeConnection | null = null;
  let context: DynamicReference | null = null;
  let catalog: DynamicReference | null = null;
  let detach: () => void = noop;
  let unregister: () => void = noop;
  let invoking = false;
  const current = (version: number) =>
    !closed && epoch === version && session?.isCurrent() === true;

  function reset() {
    epoch += 1;
    detach();
    unregister();
    detach = noop;
    unregister = noop;
    context?.release();
    catalog?.release();
    context = null;
    catalog = null;
    session = null;
    invoking = false;
  }
  function publish(version: number) {
    if (context !== null && current(version)) {
      const view = contextView(context.state.value);
      state.update((value) => ({ ...value, view }));
    }
  }
  function pending(): Pending | undefined {
    const value = state.getSnapshot();
    return value.pending.find(
      (item) => same(item, value.selection) && item.principal === session?.principal,
    );
  }
  function ready(expected: Selection | undefined, version: number) {
    const value = state.getSnapshot();
    return (
      current(version) &&
      value.phase === "ready" &&
      same(expected ?? value.selection, value.selection)
    );
  }
  function capture(expected?: Selection) {
    const value = state.getSnapshot();
    const peer = catalog;
    const selected = value.selection;
    const version = epoch;
    if (invoking || peer === null || selected === null || !ready(expected, version)) {
      throw new Error("Connect this conversation's node before compacting context.");
    }
    const previous = pending();
    if (previous === undefined && value.view?.canCompact !== true) {
      throw new Error(value.view?.unavailable ?? "Context compaction is unavailable.");
    }
    const principal = session?.principal;
    if (principal === undefined) {
      throw new Error("The node session ended.");
    }
    const fence = previous?.fence ?? value.view?.fence;
    if (fence === undefined || fence === null) {
      throw new Error("The node has not supplied an admission fence.");
    }
    const request: Pending = previous ?? {
      ...selected,
      principal,
      operation: crypto.randomUUID(),
      fence,
    };
    return { peer, version, request, previous };
  }
  async function compact(expected?: Selection) {
    let captured: ReturnType<typeof capture>;
    try {
      captured = capture(expected);
    } catch (error: unknown) {
      state.update((old) => ({ ...old, error: errorMessage(error) }));
      throw error;
    }
    const { peer, version, request, previous } = captured;
    invoking = true;
    let received = false;
    state.update((old) => ({
      ...old,
      busy: true,
      error: null,
      pending: previous === undefined ? [...old.pending, request] : old.pending,
    }));
    try {
      // Explicit checks repeat the same semantic operation identity. Lost replies
      // never create another identity or dispatch through a fallback method.
      const result = receipt(
        await call(peer, "invoke", [
          {
            command: compactCommand,
            operation: request.operation,
            arguments: { fence: request.fence },
          },
        ]),
        request.operation,
      );
      if (!current(version)) {
        throw new Error("The node changed. Check this request after reconnecting.");
      }
      received = true;
      state.update((old) => ({
        ...old,
        view: result.view,
        pending:
          result.admission === "unknown"
            ? old.pending
            : old.pending.filter(
                (item) =>
                  !(
                    same(item, request) &&
                    item.principal === request.principal &&
                    item.operation === request.operation
                  ),
              ),
        error:
          result.reason ??
          (result.admission === "unknown"
            ? "Admission is uncertain. Check this request before trying another."
            : null),
      }));
      if (result.admission === "unknown" || result.admission === "refused") {
        throw new Error(result.reason ?? "Compaction admission remains uncertain.");
      }
    } catch (error: unknown) {
      if (!received && current(version)) {
        state.update((old) => ({
          ...old,
          error:
            "The request could not be confirmed. Check the same request to recognize its outcome.",
        }));
      }
      throw error;
    } finally {
      if (epoch === version) {
        invoking = false;
        state.update((old) => ({ ...old, busy: false }));
      }
    }
  }
  async function open(selected: Selection, version: number) {
    let acquired: DynamicReference[] = [];
    try {
      const connection = await connections.open(selected.nodeId);
      if (closed || epoch !== version) {
        return;
      }
      session = connection;
      // Independent acquisitions must release even when one capability is absent.
      const results = await Promise.allSettled([
        connection.get("swarmie.context", selected.conversationId),
        connection.get("swarmie.commands", selected.conversationId),
      ]);
      acquired = results.flatMap((result) => (result.status === "fulfilled" ? [result.value] : []));
      const failure = results.find((result) => result.status === "rejected");
      if (failure?.status === "rejected") {
        throw failure.reason;
      }
      if (!current(version)) {
        return;
      }
      const [contextPeer, commandPeer] = acquired;
      if (contextPeer === undefined || commandPeer === undefined) {
        throw new Error("Context is unavailable on this node.");
      }
      const [snapshot, listed] = await Promise.all([
        call(contextPeer, "inspect"),
        call(commandPeer, "list"),
      ]);
      if (!current(version)) {
        return;
      }
      const view = contextView(snapshot);
      const alias = compactAlias(listed);
      context = contextPeer;
      catalog = commandPeer;
      acquired = [];
      detach = context.state.subscribe(() => {
        try {
          publish(version);
        } catch (error: unknown) {
          if (current(version)) {
            state.update((old) => ({ ...old, phase: "unavailable", error: errorMessage(error) }));
          }
        }
      });
      if (alias !== null) {
        unregister = commands.registerSlashCommand({
          id: compactCommand,
          owner: "context",
          command: alias,
          label: "Compact context",
          icon: CompactIcon,
          enabled: (input) =>
            ready({ nodeId: input.nodeId, conversationId: input.conversationId }, version) &&
            !invoking &&
            (state.getSnapshot().view?.canCompact === true || pending() !== undefined),
          execute: async (input) => {
            input.signal.throwIfAborted();
            await compact({ nodeId: input.nodeId, conversationId: input.conversationId });
          },
        });
      }
      state.update((old) => ({ ...old, selection: selected, phase: "ready", view, error: null }));
    } catch (error: unknown) {
      if (!closed && epoch === version) {
        state.update((old) => ({ ...old, phase: "unavailable", error: errorMessage(error) }));
      }
    } finally {
      for (const peer of acquired) {
        peer.release();
      }
    }
  }
  function changed() {
    if (closed) {
      return;
    }
    const selected = selection.getSnapshot();
    const node = selected === null ? undefined : nodes.getSnapshot().sessions[selected.nodeId];
    const principal =
      nodes.getSnapshot().nodes.find((item) => item.id === selected?.nodeId)?.principal ?? "";
    const next = JSON.stringify([selected, principal, node?.generation, node?.phase]);
    if (next === key) {
      return;
    }
    key = next;
    reset();
    state.update((old) => ({
      ...old,
      selection: selected,
      phase: node?.phase === "connected" ? "loading" : "unavailable",
      view: null,
      busy: false,
      error: null,
    }));
    if (selected !== null && node?.phase === "connected") {
      open(selected, epoch).catch(noop);
    }
  }
  const removeSelection = selection.subscribe(changed);
  const removeNodes = nodes.subscribe(changed);
  changed();
  return {
    state,
    compact,
    hasPending: () => pending() !== undefined,
    close() {
      closed = true;
      removeSelection();
      removeNodes();
      reset();
    },
  };
}

export type ContextPresentation = ReturnType<typeof createContextPresentation>;
