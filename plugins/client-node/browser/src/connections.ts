import { NodeConnectionError, connectionFailure } from "./errors";
import { NodeConnection } from "./node-connection";
import { ConnectionFailureKind, ConnectionPhase, NodeProtocol } from "./types";
import type { Connector, Node, NodeConnectionState, NodeSession } from "./types";

type Attempt = {
  generation: number;
  controller: AbortController;
  session: NodeSession | null;
  failed: boolean;
};
type Entry = {
  node: Node;
  generation: number;
  state: NodeConnectionState;
  attempt: Attempt | null;
  connection: NodeConnection | null;
  pending: Promise<NodeConnection> | null;
  retry: ReturnType<typeof setTimeout> | null;
  retryCount: number;
};

/** Owns per-node recovery only. The SDK owns RPC; consumers own references and caches. */
export class Connections {
  private readonly entries = new Map<string, Entry>();
  private readonly listeners = new Set<(state: NodeConnectionState) => void>();

  constructor(
    private readonly connector: Connector,
    private readonly changed: (state: NodeConnectionState) => void,
  ) {}

  subscribe(listener: (state: NodeConnectionState) => void) {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  }

  configure(node: Node) {
    if (
      node.id.length === 0 ||
      new TextEncoder().encode(node.id).byteLength > 128 ||
      /\s/u.test(node.id) ||
      node.principal.trim() === ""
    ) {
      throw new NodeConnectionError(
        ConnectionFailureKind.Configuration,
        "Provide a valid node identity and expected user.",
      );
    }
    const previous = this.entries.get(node.id);
    if (previous) {
      const scopeChanged = previous.node.principal !== node.principal;
      this.disconnect(node.id);
      previous.node = { ...node };
      this.publish(
        previous,
        ConnectionPhase.Disconnected,
        scopeChanged
          ? {
              kind: ConnectionFailureKind.ScopeChanged,
              message: "The node identity scope changed.",
            }
          : null,
      );
      return;
    }
    const entry: Entry = {
      node: { ...node },
      generation: 0,
      state: { nodeId: node.id, phase: ConnectionPhase.Disconnected, generation: 0, failure: null },
      attempt: null,
      connection: null,
      pending: null,
      retry: null,
      retryCount: 0,
    };
    this.entries.set(node.id, entry);
    this.publish(
      entry,
      node.url === null ? ConnectionPhase.Unconfigured : ConnectionPhase.Disconnected,
    );
  }

  state(nodeId: string) {
    return this.entry(nodeId).state;
  }

  async open(nodeId: string): Promise<NodeConnection> {
    const entry = this.entry(nodeId);
    if (entry.connection !== null && entry.connection.isCurrent()) {
      return entry.connection;
    }
    if (entry.pending) {
      return await entry.pending;
    }
    if (entry.retry !== null) {
      clearTimeout(entry.retry);
      entry.retry = null;
    }
    const attempt: Attempt = {
      generation: entry.generation + 1,
      controller: new AbortController(),
      session: null,
      failed: false,
    };
    entry.generation = attempt.generation;
    entry.attempt = attempt;
    this.publish(
      entry,
      entry.retryCount ? ConnectionPhase.Reconnecting : ConnectionPhase.Connecting,
    );
    const timeout = setTimeout(() => {
      this.fail(
        entry,
        attempt,
        new NodeConnectionError(ConnectionFailureKind.Network, "Node readiness timed out."),
      );
    }, 10_000);
    const cancelled = Promise.withResolvers<NodeConnection>();
    const aborted = () => {
      const reason: unknown = attempt.controller.signal.reason;
      cancelled.reject(
        reason instanceof Error
          ? reason
          : new NodeConnectionError(ConnectionFailureKind.Network, "The connection attempt ended."),
      );
    };
    attempt.controller.signal.addEventListener("abort", aborted, { once: true });
    const pending = Promise.race([this.establish(entry, attempt), cancelled.promise]).finally(
      () => {
        attempt.controller.signal.removeEventListener("abort", aborted);
        clearTimeout(timeout);
        if (entry.attempt === attempt) {
          entry.pending = null;
        }
      },
    );
    entry.pending = pending;
    return await pending;
  }

  private async establish(entry: Entry, attempt: Attempt) {
    try {
      const session = await this.connector(entry.node, attempt.controller.signal, (error) => {
        this.fail(entry, attempt, error);
      });
      attempt.session = session;
      if (entry.attempt !== attempt || attempt.controller.signal.aborted) {
        session.close();
        throw new NodeConnectionError(
          ConnectionFailureKind.Network,
          "The connection attempt ended.",
        );
      }
      const { identity } = session;
      if (identity.nodeId !== entry.node.id) {
        throw new NodeConnectionError(
          ConnectionFailureKind.Identity,
          "The endpoint belongs to a different node.",
        );
      }
      if (identity.principal !== entry.node.principal) {
        throw new NodeConnectionError(
          ConnectionFailureKind.Principal,
          "The node user does not match the configured identity.",
        );
      }
      if (identity.protocolVersion !== NodeProtocol.Version) {
        throw new NodeConnectionError(
          ConnectionFailureKind.Version,
          "The node protocol version is incompatible.",
        );
      }
      const connection = new NodeConnection(
        entry.node.id,
        identity.principal,
        attempt.generation,
        session,
        () => entry.attempt === attempt && !attempt.failed && !attempt.controller.signal.aborted,
      );
      entry.connection = connection;
      entry.retryCount = 0;
      this.publish(entry, ConnectionPhase.Connected);
      return connection;
    } catch (error: unknown) {
      this.fail(entry, attempt, error);
      throw error;
    }
  }

  private fail(entry: Entry, attempt: Attempt, error: unknown) {
    if (entry.attempt !== attempt || attempt.failed) {
      return;
    }
    attempt.failed = true;
    entry.connection = null;
    attempt.controller.abort(error);
    attempt.session?.close();
    const failure = connectionFailure(error);
    const retryable = failure.kind === ConnectionFailureKind.Network;
    this.publish(entry, retryable ? ConnectionPhase.Reconnecting : ConnectionPhase.Failed, failure);
    if (retryable && entry.retry === null) {
      const delay = Math.min(8000, 250 * 2 ** Math.min(entry.retryCount, 5));
      entry.retryCount += 1;
      entry.retry = setTimeout(() => {
        entry.retry = null;
        if (entry.attempt === attempt) {
          // A failed connector may ignore cancellation; its old promise cannot block recovery.
          entry.pending = null;
          this.open(entry.node.id).catch(() => undefined);
        }
      }, delay);
    }
  }

  disconnect(nodeId: string) {
    const entry = this.entry(nodeId);
    const { attempt } = entry;
    entry.generation += 1;
    entry.attempt = null;
    entry.connection = null;
    entry.pending = null;
    entry.retryCount = 0;
    if (entry.retry !== null) {
      clearTimeout(entry.retry);
      entry.retry = null;
    }
    attempt?.controller.abort();
    attempt?.session?.close();
    this.publish(entry, ConnectionPhase.Disconnected);
  }

  close() {
    for (const nodeId of this.entries.keys()) {
      this.disconnect(nodeId);
    }
  }

  private publish(
    entry: Entry,
    phase: NodeConnectionState["phase"],
    failure: NodeConnectionState["failure"] = null,
  ) {
    entry.state = { nodeId: entry.node.id, phase, generation: entry.generation, failure };
    this.changed(entry.state);
    for (const listener of this.listeners) {
      listener(entry.state);
    }
  }

  private entry(nodeId: string) {
    const entry = this.entries.get(nodeId);
    if (!entry) {
      throw new NodeConnectionError(
        ConnectionFailureKind.Configuration,
        "Configure this node before connecting.",
      );
    }
    return entry;
  }
}
