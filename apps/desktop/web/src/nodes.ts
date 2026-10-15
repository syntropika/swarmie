import type {
  LocalStoreIdentity,
  RuntimeIdentity,
  SshReadiness,
  NodeStartup,
} from "@swarmie/contracts";

import { startAction, errorMessage } from "./action-boundary";
import { responseResult } from "./checked-json";
import { clientRequest, nativeClient, observeClient } from "./client-transport";
import { decodeClientResult } from "./connections";
import type { ConnectionMethodResults } from "./connections";
import { nodeKey, sameNode, validNodeIdentity } from "./node-scope";
import { isRecord } from "./protocol";
import { hasText } from "./value-presence";
import { WorkspaceStore } from "./workspace";
import type { Workspace } from "./workspace";

export type NodeRoute = {
  name: string;
  transport: {
    kind: string;
    url?: string;
    host?: string;
    socketPath?: string;
    path?: string;
  };
  authenticated: boolean;
  revision: number;
};
export type ClientNode = {
  identity: LocalStoreIdentity;
  displayName: string;
  local: boolean;
  registered: boolean;
  lastVerifiedAtMs: number;
  connections: NodeRoute[];
};
export type { SshReadiness, NodeStartup } from "@swarmie/contracts";
type Catalog = {
  nodes: ClientNode[];
  selectedNodeId?: string;
};
export type NodesState = Catalog & {
  store: WorkspaceStore;
  error: string;
  loading: boolean;
};

// One state instance per retained node. Selection changes observation, never a chat's destination.
export class NodeClient {
  private readonly local: WorkspaceStore;
  private readonly stores = new Map<string, WorkspaceStore>();
  private readonly listeners = new Set<() => void>();
  private state: NodesState;
  private timer?: ReturnType<typeof setInterval>;
  private observing = false;
  private readonly disabled = new Set<string>();
  private selectionWrites: Promise<void> = Promise.resolve();
  private token = "";
  private startPending?: Promise<void>;
  private requestId = 0;
  private running = false;
  private lifecycle = 0;
  private catalogLoaded = false;
  private catalogRetryAt = 0;
  private catalogPending?: Promise<void>;
  private catalogNeedsRefresh = true;
  private async control<M extends keyof ConnectionMethodResults>(
    method: M,
    params: unknown = {},
    timeout = 30_000,
  ): Promise<ConnectionMethodResults[M]> {
    const request = async (
      path: string,
      requestMethod: string,
      requestParams: unknown,
      target?: RuntimeIdentity,
    ) => {
      const response = await clientRequest(
        path,
        {
          jsonrpc: "2.0",
          id: (this.requestId += 1),
          method: requestMethod,
          params: requestParams,
          ...(target
            ? {
                expectedRuntime: target,
              }
            : {}),
        },
        this.token,
        path === "/client/rpc" ? timeout : 30_000,
      );
      if (!response.ok) {
        throw new Error(
          response.status === 401
            ? "Enter the local connection token, then retry."
            : `Node management returned HTTP ${response.status}.`,
        );
      }
      const value: unknown = await response.json();
      return responseResult(value);
    };
    const initialized = decodeClientResult(
      "runtime/initialize",
      await request("/rpc", "runtime/initialize", {}),
    );
    const identity = initialized.runtimeIdentity;
    const local = this.local.getSnapshot().node;
    if (
      initialized.name !== "swarmie" ||
      initialized.protocolVersion !== "1" ||
      !validNodeIdentity(identity?.identity) ||
      !local ||
      !sameNode(local, identity.identity)
    ) {
      throw new Error(
        "The local connection now serves a different node. Saved work has been kept.",
      );
    }
    return decodeClientResult(method, await request("/client/rpc", method, params, identity));
  }
  constructor(local = new WorkspaceStore()) {
    this.local = local;
    this.state = {
      nodes: [],
      store: local,
      error: "",
      loading: false,
    };
    local.subscribe(() => {
      this.publish({});
    });
  }
  getSnapshot = () => this.state;
  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };
  private publish(patch: Partial<NodesState>) {
    this.state = {
      ...this.state,
      ...patch,
    };
    for (const listener of this.listeners) {
      listener();
    }
  }
  private store(node: ClientNode): WorkspaceStore {
    const key = nodeKey(node.identity);
    let store = this.stores.get(key);
    if (!store) {
      const local = this.local.getSnapshot().node;
      store =
        local && sameNode(local, node.identity)
          ? this.local
          : new WorkspaceStore({
              node: node.identity,
              rpcPath: `/nodes/${encodeURIComponent(node.identity.environmentId)}/rpc`,
            });
      this.stores.set(key, store);
      if (store !== this.local) {
        store.subscribe(() => {
          this.publish({});
        });
      }
    }
    return store;
  }
  private reportError(error: unknown) {
    this.publish({ error: errorMessage(error) });
  }
  private startObservation() {
    if (nativeClient()) {
      startAction(this.observeNative(), this.reportError.bind(this));
      return;
    }
    if (this.timer || !this.running) {
      return;
    }
    this.timer = setInterval(() => {
      const { lifecycle } = this;
      if (!this.local.getSnapshot().connected) {
        this.catalogNeedsRefresh = true;
      }
      startAction(this.pollLocal(lifecycle), this.local.setError.bind(this.local));
      for (const store of new Set(this.stores.values())) {
        if (store !== this.local) {
          startAction(store.poll(), store.setError.bind(store));
        }
      }
    }, 500);
  }
  private async pollLocal(lifecycle: number) {
    await this.local.poll();
    if (lifecycle !== this.lifecycle || !this.running || !this.local.getSnapshot().connected) {
      return;
    }
    if ((!this.catalogLoaded || this.catalogNeedsRefresh) && Date.now() >= this.catalogRetryAt) {
      await this.loadCatalog();
    }
  }
  private async applyObservation(
    store: WorkspaceStore,
    events: Awaited<ReturnType<typeof observeClient>>["events"],
  ) {
    if (this.disabled.has(store.getSnapshot().node?.environmentId ?? "")) {
      return;
    }
    const relevant = events.filter(
      (event) =>
        !hasText(event.environmentId) ||
        store.getSnapshot().node?.environmentId === event.environmentId,
    );
    for (const event of relevant) {
      const connected =
        event.method === "client/connected" ||
        (event.method === "client/status" &&
          isRecord(event.params) &&
          event.params.connected === true);
      const disconnected =
        event.method === "client/disconnected" ||
        (event.method === "client/status" &&
          !(isRecord(event.params) && event.params.connected === true));
      if (disconnected && store.getSnapshot().connected) {
        store.observeDisconnected();
      } else if (connected && !store.getSnapshot().connected) {
        // oxlint-disable-next-line eslint/no-await-in-loop -- A connection must settle before a later disconnect event for the same store is applied.
        await store.connect(this.token);
      }
    }
    if (store.getSnapshot().connected) {
      await store.poll();
    }
  }
  private async observeNative() {
    if (this.observing || !this.running) {
      return;
    }
    this.observing = true;
    const { lifecycle } = this;
    try {
      while (this.running && lifecycle === this.lifecycle) {
        const {
          events,
          // oxlint-disable-next-line eslint/no-await-in-loop -- Native long-poll observation owns one cursor; concurrent requests would reorder the connection stream.
        } = await observeClient();
        if (!this.running || lifecycle !== this.lifecycle) {
          return;
        }
        for (const store of new Set([this.local, ...this.stores.values()])) {
          // Each store consumes native connection events before polling that same session cursor.
          // oxlint-disable-next-line eslint/no-await-in-loop -- Reconnect and polling must complete in event order before the next observer batch.
          await this.applyObservation(store, events);
        }
        if (
          (!this.catalogLoaded || this.catalogNeedsRefresh) &&
          this.local.getSnapshot().connected
        ) {
          // oxlint-disable-next-line eslint/no-await-in-loop -- Catalog refresh belongs to this completed connection batch before the next long poll.
          await this.loadCatalog();
        }
      }
    } catch (error) {
      if (this.running && lifecycle === this.lifecycle) {
        this.publish({
          error: String(error),
        });
      }
    } finally {
      this.observing = false;
      if (this.running && lifecycle !== this.lifecycle) {
        this.startObservation();
      }
    }
  }
  private async loadCatalog() {
    if (this.catalogPending) {
      await this.catalogPending;
      return;
    }
    const pending = this.refresh(!this.catalogLoaded).finally(() => {
      if (this.catalogPending === pending) {
        this.catalogPending = undefined;
      }
    });
    this.catalogPending = pending;
    await pending;
  }
  async start(): Promise<void> {
    this.running = true;
    this.startObservation();
    if (this.startPending) {
      await this.startPending;
      return;
    }
    const { lifecycle } = this;
    const pending = (async () => {
      await this.local.connect(this.token);
      if (lifecycle !== this.lifecycle || !this.running || !this.local.getSnapshot().connected) {
        return;
      }
      await this.loadCatalog();
    })().finally(() => {
      if (this.startPending === pending) {
        this.startPending = undefined;
      }
    });
    this.startPending = pending;
    await pending;
  }
  async refresh(restoreSelection = false) {
    const { lifecycle } = this;
    this.publish({
      loading: true,
      error: "",
    });
    try {
      const catalog = await this.control("nodes/list");
      if (lifecycle !== this.lifecycle) {
        return;
      }
      this.catalogLoaded = true;
      this.catalogNeedsRefresh = false;
      this.catalogRetryAt = 0;
      for (const node of catalog.nodes) {
        const store = this.store(node);
        if (!node.registered) {
          store.disconnect();
        }
      }
      const selected = restoreSelection
        ? catalog.nodes.find((node) => node.identity.environmentId === catalog.selectedNodeId)
        : undefined;
      const store = selected ? this.store(selected) : this.state.store;
      this.publish({
        ...catalog,
        store,
      });
      if (selected && selected.registered && store !== this.local) {
        await store.connect(this.token);
      }
    } catch (error) {
      if (lifecycle !== this.lifecycle) {
        return;
      }
      this.catalogRetryAt = Date.now() + 5000;
      this.publish({
        error: error instanceof Error ? error.message : String(error),
      });
    } finally {
      if (lifecycle === this.lifecycle) {
        this.publish({
          loading: false,
        });
      }
    }
  }
  async select(environmentId: string) {
    this.disabled.delete(environmentId);
    const { lifecycle } = this;
    const node = this.state.nodes.find(
      (candidate) => candidate.identity.environmentId === environmentId,
    );
    if (!(node?.registered === true)) {
      throw new Error("This node is no longer registered. Add a connection to use it again.");
    }
    const store = this.store(node);
    this.publish({
      store,
      selectedNodeId: environmentId,
      error: "",
    });
    const save = (async () => {
      try {
        await this.selectionWrites;
      } catch {
        // A previous selection failure must not block this new choice.
      }
      await this.control("nodes/select", { environmentId });
    })();
    this.selectionWrites = save;
    try {
      await save;
    } catch (error) {
      if (lifecycle === this.lifecycle && this.state.store === store) {
        this.publish({
          error: String(error),
        });
      }
    }
    if (lifecycle !== this.lifecycle) {
      return;
    }
    if (!store.getSnapshot().connected) {
      await store.connect(this.token);
    }
  }
  async reconnect(token?: string) {
    const { lifecycle } = this;
    if (token !== undefined) {
      this.token = token;
    }
    await this.local.connect(this.token);
    if (lifecycle !== this.lifecycle || !this.local.getSnapshot().connected) {
      return;
    }
    await this.refresh(!this.catalogLoaded);
    if (lifecycle !== this.lifecycle) {
      return;
    }
    if (this.state.store !== this.local) {
      await this.state.store.connect(this.token);
    }
    this.startObservation();
  }
  localSnapshot = (): Workspace => this.local.getSnapshot();
  nodeSnapshot(node: ClientNode): Workspace {
    return this.store(node).getSnapshot();
  }
  async probe(environmentId: string, harness: string) {
    const node = this.state.nodes.find(
      (candidate) => candidate.identity.environmentId === environmentId,
    );
    if (!(node?.registered === true)) {
      throw new Error("This node has no active connection.");
    }
    return await this.store(node).probe(harness);
  }
  async pair(name: string, url: string, token: string) {
    await this.control("nodes/pair", {
      name,
      url,
      token,
    });
    await this.refresh();
  }
  async inspectSsh(host: string, socketPath: string): Promise<SshReadiness> {
    return await this.control(
      "nodes/inspectSsh",
      {
        host,
        socketPath,
      },
      90_000,
    );
  }
  async prepareSsh(
    name: string,
    host: string,
    socketPath: string,
    action: "install" | "start",
  ): Promise<SshReadiness> {
    const result = await this.control(
      "nodes/prepareSsh",
      {
        name,
        host,
        socketPath,
        action,
      },
      660_000,
    );
    await this.refresh();
    return result;
  }
  async startup(node: ClientNode, enabled?: boolean, boot = false): Promise<NodeStartup> {
    const route = node.connections.find(
      (candidate) => candidate.transport.kind === "ssh" && !hasText(candidate.transport.socketPath),
    );
    return await this.control(
      "nodes/startup",
      {
        environmentId: node.identity.environmentId,
        name: route?.name,
        revision: route?.revision,
        ...(enabled === undefined
          ? {}
          : {
              enabled,
            }),
        boot,
      },
      90_000,
    );
  }
  async pairSsh(name: string, host: string, socketPath: string, runtimeIdentity: RuntimeIdentity) {
    await this.control("nodes/pairSsh", {
      name,
      host,
      socketPath,
      runtimeIdentity,
    });
    await this.refresh();
  }
  async removeRoute(name: string, revision: number) {
    await this.control("nodes/removeRoute", {
      name,
      revision,
    });
    await this.refresh();
  }
  async connect(environmentId: string) {
    this.disabled.delete(environmentId);
    const node = this.state.nodes.find(
      (candidate) => candidate.identity.environmentId === environmentId,
    );
    if (!(node?.registered === true)) {
      throw new Error("Add a connection before reconnecting this node.");
    }
    await this.store(node).connect(this.token);
  }
  disconnect(environmentId: string) {
    const node = this.state.nodes.find(
      (candidate) => candidate.identity.environmentId === environmentId,
    );
    if (node && !node.local) {
      this.disabled.add(environmentId);
      this.store(node).disconnect();
      if (nativeClient()) {
        startAction(
          this.control("nodes/disconnect", { environmentId }),
          this.reportError.bind(this),
        );
      }
    }
  }
  status(node: ClientNode): string {
    const state = this.store(node).getSnapshot();
    if (!node.registered) {
      return "Removed";
    }
    if (state.connected) {
      return "Connected";
    }
    if (state.busy) {
      return "Connecting…";
    }
    return state.error.length > 0 ? "Unavailable" : "Not connected";
  }
  error(node: ClientNode): string {
    return this.store(node).getSnapshot().error;
  }
  dispose() {
    this.running = false;
    this.lifecycle += 1;
    this.startPending = undefined;
    this.catalogPending = undefined;
    clearInterval(this.timer);
    this.timer = undefined;
    for (const store of new Set([this.local, ...this.stores.values()])) {
      store.disconnect();
    }
  }
}
