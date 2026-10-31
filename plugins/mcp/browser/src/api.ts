import type { Snapshot, UiStore } from "@swarmie/client-ui";
import type {
  Connections,
  ConnectionsState,
  NodeConnection,
} from "@swarmie/plugin-browser/connections";
import type { DynamicReference } from "@syntropika/tembra-sdk";

import type { McpDefinition, McpState } from "./types";
import { servers, validateDefinition } from "./validation";

function message(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}
async function invoke(peer: DynamicReference, method: string, input?: unknown): Promise<unknown> {
  const call: unknown = Reflect.get(peer, method);
  if (typeof call !== "function") {
    throw new TypeError("This node does not support MCP configuration.");
  }
  return await Reflect.apply(call, undefined, input === undefined ? [] : [input]);
}

/** Each backend reference stays bound to one verified node, principal and generation. */
export function createMcpApi(
  state: UiStore<Readonly<McpState>>,
  nodes: Snapshot<Readonly<ConnectionsState>>,
  connections: Connections,
) {
  let closed = false;
  let epoch = 0;
  let binding = "";
  let reference: DynamicReference | null = null;
  let session: NodeConnection | null = null;
  let pending: Promise<void> | null = null;
  let changing = false;
  const current = (version: number) =>
    !closed && version === epoch && session?.isCurrent() === true;
  function selected() {
    const value = nodes.getSnapshot();
    return value.nodes.find((node) => node.id === value.selected);
  }
  function reset() {
    epoch += 1;
    reference?.release();
    reference = null;
    session = null;
    pending = null;
    changing = false;
    const node = selected();
    state.set({
      servers: [],
      loading: false,
      busy: null,
      error: null,
      available: false,
      nodeId: node?.id ?? null,
      principal: node?.principal ?? null,
    });
  }
  function changed() {
    const node = selected();
    const phase = node ? nodes.getSnapshot().sessions[node.id] : undefined;
    const next = `${node?.id ?? ""}:${node?.principal ?? ""}:${phase?.generation ?? 0}:${phase?.phase ?? ""}`;
    if (next === binding) {
      return;
    }
    binding = next;
    reset();
    if (phase?.phase === "connected") {
      refresh().catch(() => undefined);
    }
  }
  async function refresh(): Promise<void> {
    if (closed) {
      return;
    }
    if (pending) {
      await pending;
      return;
    }
    const node = selected();
    if (!node || nodes.getSnapshot().sessions[node.id]?.phase !== "connected") {
      reset();
      return;
    }
    const version = epoch;
    state.update((value) => ({ ...value, loading: true, error: null }));
    const operation = async () => {
      try {
        const connected = await connections.open(node.id);
        if (closed || version !== epoch) {
          return;
        }
        session = connected;
        const peer = reference ?? (await connected.get("swarmie.mcp.settings", "main"));
        if (!current(version)) {
          peer.release();
          return;
        }
        reference = peer;
        const result = servers(await invoke(peer, "list"));
        if (current(version)) {
          state.update((value) => ({ ...value, servers: result, available: true, loading: false }));
        }
      } catch (error: unknown) {
        if (!closed && version === epoch) {
          state.update((value) => ({ ...value, loading: false, error: message(error) }));
        }
      }
    };
    pending = operation();
    try {
      await pending;
    } finally {
      if (version === epoch) {
        pending = null;
      }
    }
  }
  async function change(method: string, definition: McpDefinition, input: unknown) {
    if (closed || changing) {
      throw new Error("Wait for the current MCP action to finish.");
    }
    const peer = reference;
    const version = epoch;
    if (!peer || !current(version)) {
      throw new Error("Connect the execution node before managing MCP servers.");
    }
    changing = true;
    state.update((value) => ({ ...value, busy: definition.id, error: null }));
    let failure: unknown;
    try {
      await invoke(peer, method, input);
    } catch (error: unknown) {
      failure = error;
    }
    if (current(version)) {
      // Reconcile by reading; a mutation with an unknown outcome is never replayed.
      try {
        const result = servers(await invoke(peer, "list"));
        if (current(version)) {
          state.update((value) => ({ ...value, servers: result }));
        }
      } catch (error: unknown) {
        failure ??= error;
      }
      if (current(version)) {
        changing = false;
        state.update((value) => ({
          ...value,
          busy: null,
          error: failure === undefined ? null : message(failure),
        }));
      }
    }
    if (!current(version)) {
      throw new Error("The execution node changed. Refresh to confirm the result.");
    }
    if (failure !== undefined) {
      throw failure instanceof Error ? failure : new Error(message(failure));
    }
  }
  const detach = nodes.subscribe(changed);
  changed();
  return {
    state,
    refresh,
    async save(definition: McpDefinition, expectedRevision: number | null) {
      validateDefinition(definition);
      await change("save", definition, { definition, expected_revision: expectedRevision });
    },
    async setEnabled(definition: McpDefinition, enabled: boolean) {
      await change(enabled ? "enable" : "disable", definition, {
        id: definition.id,
        revision: definition.revision,
      });
    },
    async remove(definition: McpDefinition) {
      await change("remove", definition, { id: definition.id, revision: definition.revision });
    },
    dispose() {
      closed = true;
      detach();
      reset();
    },
  };
}
