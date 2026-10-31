import type { Snapshot } from "@swarmie/client-ui";
import type { DynamicReference } from "@syntropika/tembra-sdk";

import type { Connections } from "./connections";
import type { NodeConnection } from "./node-connection";
import { readPagedSnapshot } from "./paging";
import type { ConnectionsState } from "./types";

/** Owns one selected service reference. The SDK owns RPC and State subscriptions. */
export function observeNodeService(
  nodes: Snapshot<Readonly<ConnectionsState>>,
  connections: Connections,
  service: string,
  observer: {
    reset: (nodeId: string | null, principal: string | null) => void;
    receive: (value: unknown) => void;
    failed: (error: unknown) => void;
    ready: () => void;
  },
) {
  let closed = false;
  let epoch = 0;
  let binding = "";
  let reference: DynamicReference | null = null;
  let session: NodeConnection | null = null;
  let detach: (() => void) | null = null;
  let notice: string | number | null = null;
  let reading: { minimum: number; id: number; promise: Promise<unknown> } | null = null;
  let sequence = 0;
  let floor = 0;
  let dirty = false;
  const current = (version: number) =>
    !closed && epoch === version && session?.isCurrent() === true;
  async function call(peer: DynamicReference, method: string, input?: unknown): Promise<unknown> {
    const operation: unknown = Reflect.get(peer, method);
    if (typeof operation !== "function") {
      throw new TypeError(`The node does not support ${service}.${method}.`);
    }
    return await Reflect.apply(operation, undefined, input === undefined ? [] : [input]);
  }
  async function open(nodeId: string, version: number) {
    let acquired: DynamicReference | null = null;
    try {
      const connected = await connections.open(nodeId);
      if (closed || epoch !== version) {
        return;
      }
      session = connected;
      acquired = await connected.get(service, "main");
      if (!current(version)) {
        return;
      }
      reference = acquired;
      acquired = null;
      const peer = reference;
      const report = (error: unknown) => {
        if (current(version)) {
          observer.failed(error);
        }
      };
      detach = peer.state.subscribe(() => {
        if (current(version)) {
          const value: unknown = peer.state.value;
          const raw =
            value !== null && typeof value === "object" && "revision" in value
              ? value.revision
              : null;
          const next = typeof raw === "string" || typeof raw === "number" ? raw : null;
          if (notice !== next) {
            notice = next;
            dirty = true;
            refresh(peer, version).catch(report);
          }
        }
      });
      await refresh(peer, version);
    } catch (error: unknown) {
      if (!closed && epoch === version) {
        detach?.();
        detach = null;
        reference?.release();
        reference = null;
        observer.failed(error);
      }
    } finally {
      acquired?.release();
    }
  }
  function changed() {
    const value = nodes.getSnapshot();
    const node = value.nodes.find((entry) => entry.id === value.selected);
    const phase = node ? value.sessions[node.id] : undefined;
    const next = JSON.stringify([node?.id, node?.principal, phase?.generation, phase?.phase]);
    if (next === binding) {
      return;
    }
    binding = next;
    epoch += 1;
    detach?.();
    detach = null;
    reference?.release();
    reference = null;
    session = null;
    reading = null;
    floor = 0;
    observer.reset(node?.id ?? null, node?.principal ?? null);
    if (node && phase?.phase === "connected") {
      open(node.id, epoch).catch(observer.failed);
    }
  }
  async function refresh(
    peer: DynamicReference,
    version: number,
    minimum = floor,
  ): Promise<unknown> {
    const required = Math.max(floor, minimum);
    if (reading !== null && reading.minimum >= required) {
      return await reading.promise;
    }
    sequence += 1;
    const id = sequence;
    const active = () => current(version) && id === sequence;
    const operation = async (): Promise<unknown> => {
      while (true) {
        dirty = false;
        try {
          // oxlint-disable-next-line no-await-in-loop -- Revision changes restart only the dependent read projection, never a mutation.
          const value = await readPagedSnapshot(peer, active, Math.max(required, floor));
          const revision =
            value !== null && typeof value === "object" && "revision" in value
              ? value.revision
              : null;
          if (typeof revision === "number" && revision < floor) {
            continue;
          }
          if (dirty && notice !== revision && active()) {
            continue;
          }
          if (active()) {
            observer.receive(value);
            observer.ready();
          }
          return value;
        } catch (error: unknown) {
          if (current(version) && id !== sequence) {
            // oxlint-disable-next-line no-await-in-loop -- A superseded projection joins a newer read; no command is repeated.
            return await refresh(peer, version, required);
          }
          if (dirty && active()) {
            continue;
          }
          throw error;
        }
      }
    };
    const promise = (async () => {
      // Install the coalesced wait before a same-host method can synchronously publish.
      await Promise.resolve();
      return await operation();
    })();
    reading = { minimum: required, id, promise };
    try {
      return await promise;
    } finally {
      if (epoch === version && reading?.id === id) {
        reading = null;
      }
    }
  }
  async function reconcile(peer: DynamicReference, version: number) {
    try {
      await refresh(peer, version);
    } catch (error: unknown) {
      if (current(version)) {
        observer.failed(error);
      }
    }
  }
  const unsubscribe = nodes.subscribe(changed);
  changed();
  return {
    async invoke(method: string, input?: unknown) {
      const version = epoch;
      const peer = reference;
      if (!peer || !current(version)) {
        throw new Error("Connect the execution node before using this capability.");
      }
      try {
        const result = await call(peer, method, input);
        if (
          current(version) &&
          result !== null &&
          typeof result === "object" &&
          "revision" in result &&
          typeof result.revision === "number" &&
          Number.isSafeInteger(result.revision)
        ) {
          floor = Math.max(floor, result.revision);
        }
        return result;
      } catch (error: unknown) {
        if (current(version)) {
          await reconcile(peer, version);
        }
        throw error;
      }
    },
    async read(minimumRevision = floor) {
      const version = epoch;
      const peer = reference;
      if (!peer || !current(version)) {
        throw new Error("Connect the execution node before reading this capability.");
      }
      return await refresh(peer, version, minimumRevision);
    },
    close() {
      closed = true;
      epoch += 1;
      unsubscribe();
      detach?.();
      reference?.release();
      reference = null;
    },
  };
}
