import type { Snapshot } from "@swarmie/client-ui";
import type {
  Connections,
  ConnectionsState,
  NodeConnection,
} from "@swarmie/plugin-connections-browser";
import type {
  Conversation,
  ConversationExecutionApi,
  Execution,
  createConversationRecords,
} from "@swarmie/plugin-conversations-browser";
import type { DynamicReference } from "@syntropika/tembra-sdk";

import {
  conversationCatalogPage,
  conversationMetadata,
  sameExecutionContext,
  conversationProjection,
  refusalReason,
} from "./conversation-values";
import { readNodePages } from "./node-pages";
import { callRemote } from "./remote";
import type { ExecutionFactory } from "./submissions";

const unavailable = "The node conversation service is unavailable.";

type Metadata = Pick<Conversation, "title" | "pinned" | "archived">;

type Binding = {
  connection: NodeConnection;
  reference: DynamicReference;
  detach: () => void;
  phase: Execution["phase"];
};

function bindingKey(connection: NodeConnection, id: string) {
  return JSON.stringify([connection.nodeId, connection.principal, connection.generation, id]);
}

function noop() {
  return undefined;
}

async function catalogEntries(reference: DynamicReference, current: () => boolean) {
  const pages = await readNodePages(
    async (cursor) => {
      const page = conversationCatalogPage(
        await callRemote(reference, "list", [cursor], unavailable),
      );
      if (!current()) {
        throw new Error("The conversation index belongs to an earlier node session.");
      }
      return page;
    },
    { reason: "conversations.catalog_changed", current },
  );
  return pages.flatMap((page) => page.entries);
}

/** The node admits managed invocations; the client only observes their Tembra projections. */
export function createNodeExecution(
  nodes: Snapshot<Readonly<ConnectionsState>>,
  connections: Connections,
  records: ReturnType<typeof createConversationRecords>,
  active: () => string,
) {
  const bindings = new Map<string, Binding>();
  const acquiring = new Map<string, Promise<Binding>>();
  const restored = new Map<string, NodeConnection>();
  const restoreListeners = new Set<() => void>();
  const activeCalls = new Map<string, number>();
  const metadataWrites = new Map<string, Promise<void>>();
  const available = (nodeId: string) => restored.get(nodeId)?.isCurrent() === true;
  function notifyRestore() {
    for (const listener of restoreListeners) {
      listener();
    }
  }

  // This count only protects observation eviction. Tembra owns the physical invocation.
  async function withBinding<T>(
    connection: NodeConnection,
    id: string,
    action: (binding: Binding) => Promise<T>,
  ) {
    const key = bindingKey(connection, id);
    activeCalls.set(key, (activeCalls.get(key) ?? 0) + 1);
    try {
      return await action(await acquire(connection, id));
    } finally {
      const remaining = (activeCalls.get(key) ?? 1) - 1;
      if (remaining === 0) {
        activeCalls.delete(key);
      } else {
        activeCalls.set(key, remaining);
      }
    }
  }
  let closed = false;
  let selected = "";
  function apply(binding: Binding) {
    if (closed || !binding.connection.isCurrent()) {
      return;
    }
    const value = conversationProjection(binding.reference.state.value);
    if (
      value.executionContext &&
      (value.executionContext.nodeId !== binding.connection.nodeId ||
        value.executionContext.principal !== binding.connection.principal)
    ) {
      throw new Error("The execution directory belongs to another node or user.");
    }
    const existing = records.roster
      .getSnapshot()
      .find((record) => record.id === value.conversationId);
    if (
      existing?.started === true &&
      existing.executionContext !== undefined &&
      !sameExecutionContext(existing.executionContext, value.executionContext)
    ) {
      throw new Error("The node changed the bound conversation directory.");
    }
    binding.phase = value.phase;
    records.update(value.conversationId, (record) => ({
      ...record,
      executionContext: value.executionContext ?? record.executionContext,
      messages: value.messages,
      execution: {
        ...record.execution,
        ownerConversationId: value.ownerConversationId,
        phase: value.phase,
        label: value.label,
        omittedMessages: value.omittedMessages,
        canAcknowledge: value.canAcknowledge,
        lifecycleWarning: value.lifecycleWarning,
      },
    }));
  }
  async function acquire(connection: NodeConnection, id: string) {
    const key = bindingKey(connection, id);
    const held = bindings.get(key);
    if (held !== undefined) {
      return held;
    }
    const existing = acquiring.get(key);
    if (existing !== undefined) {
      return await existing;
    }
    const operation = open(connection, id, key);
    acquiring.set(key, operation);
    try {
      return await operation;
    } finally {
      acquiring.delete(key);
    }
  }
  async function open(connection: NodeConnection, id: string, key: string) {
    if (new Set([...bindings.keys(), ...acquiring.keys()]).size >= 8) {
      const candidate = [...bindings].find(
        ([oldKey, old]) =>
          !activeCalls.has(oldKey) &&
          !acquiring.has(oldKey) &&
          !["working", "queued", "stopping"].includes(old.phase),
      );
      if (candidate === undefined) {
        throw new Error("The node has too many active conversation observations.");
      }
      const [oldKey, old] = candidate;
      old.detach();
      old.reference.release();
      bindings.delete(oldKey);
    }
    const reference = await connection.get("swarmie.conversation.session", id);
    if (closed || !connection.isCurrent()) {
      reference.release();
      throw new Error("The conversation belongs to an earlier node session.");
    }
    const binding: Binding = { connection, reference, detach: () => undefined, phase: "idle" };
    bindings.set(key, binding);
    binding.detach = reference.state.subscribe(() => {
      try {
        apply(binding);
      } catch (error: unknown) {
        records.fail(error);
      }
    });
    try {
      await callRemote(reference, "snapshot", [], unavailable);
      apply(binding);
      return binding;
    } catch (error) {
      binding.detach();
      reference.release();
      bindings.delete(key);
      throw error;
    }
  }
  function factory(connection: NodeConnection): ConversationExecutionApi {
    return {
      async submit(input, signal) {
        signal.throwIfAborted();
        const admitted = Promise.withResolvers<
          { accepted: true } | { accepted: false; reason: string }
        >();
        let detach: () => void = noop;
        const abort = () => {
          admitted.reject(new DOMException("Admission observation was cancelled.", "AbortError"));
        };
        const run = withBinding(connection, input.conversationId, async (binding) => {
          signal.throwIfAborted();
          if (!connection.isCurrent()) {
            throw new Error("The node session ended before submission.");
          }
          const inspect = () => {
            try {
              if (!connection.isCurrent()) {
                throw new Error("The node session changed before admission was confirmed.");
              }
              if (
                conversationProjection(binding.reference.state.value).admitted.includes(
                  input.submissionId,
                )
              ) {
                admitted.resolve({ accepted: true });
              }
            } catch (error) {
              admitted.reject(error);
            }
          };
          detach = binding.reference.state.subscribe(inspect);
          signal.addEventListener("abort", abort, { once: true });
          const result = await callRemote(binding.reference, "run", [input], unavailable);
          if (!connection.isCurrent()) {
            throw new Error("The submission result belongs to an earlier node session.");
          }
          const refusal = refusalReason(result);
          if (refusal === undefined) {
            inspect();
            if (
              !conversationProjection(binding.reference.state.value).admitted.includes(
                input.submissionId,
              )
            ) {
              admitted.reject(new Error("The node did not confirm this submission."));
            }
          } else {
            admitted.resolve({ accepted: false, reason: refusal });
          }
        });
        function failed(error: unknown) {
          admitted.reject(error);
          if (connection.isCurrent()) {
            records.fail(error);
          }
        }
        run.catch(failed);
        try {
          return await admitted.promise;
        } finally {
          detach();
          signal.removeEventListener("abort", abort);
        }
      },
      async stop(id, signal) {
        signal.throwIfAborted();
        await withBinding(connection, id, async (binding) => {
          signal.throwIfAborted();
          await callRemote(binding.reference, "stop", [], unavailable);
          if (!connection.isCurrent()) {
            throw new Error("The cancellation result belongs to an earlier node session.");
          }
        });
      },
      async acknowledge(id, signal) {
        signal.throwIfAborted();
        await withBinding(connection, id, async (binding) => {
          signal.throwIfAborted();
          await callRemote(binding.reference, "acknowledge", [], unavailable);
          if (!connection.isCurrent()) {
            throw new Error("The acknowledgement belongs to an earlier node session.");
          }
          apply(binding);
        });
      },
    };
  }
  async function restore(nodeId: string) {
    let reference: DynamicReference | null = null;
    let connection: NodeConnection | null = null;
    try {
      connection = await connections.open(nodeId);
      reference = await connection.get("swarmie.conversation.catalog", "main");
      const source = connection;
      const values = await catalogEntries(reference, () => !closed && source.isCurrent());
      if (closed || !connection.isCurrent()) {
        return;
      }
      for (const value of values) {
        if (
          value.executionContext &&
          (value.executionContext.nodeId !== connection.nodeId ||
            value.executionContext.principal !== connection.principal)
        ) {
          throw new Error("The restored execution directory belongs to another node or user.");
        }
        if (!records.roster.getSnapshot().some((record) => record.id === value.id)) {
          records.create({
            id: value.id,
            title: value.title,
            started: true,
            node: nodeId,
            executionContext: value.executionContext,
            pinned: value.pinned,
            archived: value.archived,
            draft: {
              text: "",
              attachments: [],
              account: value.account,
              model: value.model,
              effort: value.effort,
              node: nodeId,
              executionContext: value.executionContext,
            },
            messages: [],
            execution: {
              phase: "idle",
              label: "",
              queue: [],
              steering: [],
              lifecycleWarning: value.lifecycleWarning,
            },
          });
        }
      }
      restored.set(nodeId, connection);
      notifyRestore();
      const current = records.roster.getSnapshot().find((record) => record.id === active());
      if (current?.node === nodeId) {
        await acquire(connection, current.id);
      }
    } catch (error) {
      if (!closed && connection?.isCurrent() === true) {
        restored.delete(nodeId);
        notifyRestore();
        records.fail(error);
      }
    } finally {
      reference?.release();
    }
  }
  function changed() {
    for (const [nodeId, connection] of restored) {
      if (!connection.isCurrent()) {
        restored.delete(nodeId);
        notifyRestore();
      }
    }
    for (const [key, binding] of bindings) {
      if (!binding.connection.isCurrent()) {
        binding.detach();
        binding.reference.release();
        bindings.delete(key);
      }
    }
    const value = nodes.getSnapshot();
    const session = value.sessions[value.selected];
    const next = `${value.selected}:${session?.generation ?? 0}:${session?.phase ?? ""}`;
    if (next === selected) {
      return;
    }
    selected = next;
    if (session?.phase === "connected") {
      restore(value.selected).catch(records.fail);
    }
  }
  const detach = nodes.subscribe(changed);
  changed();
  return {
    limits: { maxTextScalars: 1_048_576, attachments: false },
    factory: factory satisfies ExecutionFactory,
    async observe(nodeId: string, id: string) {
      const connection = await connections.open(nodeId);
      await acquire(connection, id);
    },
    available,
    subscribe(listener: () => void) {
      restoreListeners.add(listener);
      return () => {
        restoreListeners.delete(listener);
      };
    },
    async metadata(nodeId: string, id: string, change: (value: Metadata) => Partial<Metadata>) {
      const connection = restored.get(nodeId);
      if (closed || connection?.isCurrent() !== true) {
        throw new Error(
          "The conversation catalog is not ready. Reload before editing its metadata.",
        );
      }
      const key = bindingKey(connection, id);
      const previous = metadataWrites.get(key) ?? Promise.resolve();
      // Each explicit edit runs once. A failed earlier edit is never retried.
      const write = previous
        .catch(() => undefined)
        .then(async () => {
          if (closed || !connection.isCurrent()) {
            throw new Error("The node session has ended.");
          }
          const reference = await connection.get("swarmie.conversation.catalog", "main");
          try {
            const entries = await catalogEntries(
              reference,
              () => !closed && connection.isCurrent(),
            );
            const confirmed = entries.find((entry) => entry.id === id);
            if (confirmed === undefined) {
              throw new Error("The conversation is not indexed by this node.");
            }
            const values = { ...confirmed, ...change(confirmed) };
            if (closed || !connection.isCurrent()) {
              throw new Error("The node session has ended.");
            }
            const result = conversationMetadata(
              await callRemote(
                reference,
                "update",
                [id, values.title, values.pinned, values.archived],
                unavailable,
              ),
            );
            if (result.id !== id) {
              throw new Error("The metadata confirmation belongs to another conversation.");
            }
            if (closed || !connection.isCurrent()) {
              throw new Error("The metadata result belongs to an earlier node session.");
            }
            await records.commit(id, (record) =>
              closed || !connection.isCurrent()
                ? record
                : {
                    ...record,
                    title: result.title,
                    pinned: result.pinned,
                    archived: result.archived,
                    execution: { ...record.execution, lifecycleWarning: result.lifecycleWarning },
                  },
            );
          } finally {
            reference.release();
          }
        });
      metadataWrites.set(key, write);
      try {
        await write;
      } finally {
        if (metadataWrites.get(key) === write) {
          metadataWrites.delete(key);
        }
      }
    },
    dispose() {
      closed = true;
      detach();
      for (const binding of bindings.values()) {
        binding.detach();
        binding.reference.release();
      }
      bindings.clear();
      restored.clear();
      restoreListeners.clear();
    },
  };
}
