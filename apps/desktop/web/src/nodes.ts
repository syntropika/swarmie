import { clientRequest, nativeClient, observeClient } from "./clientTransport";
import type { LocalStoreIdentity, RuntimeIdentity, SshReadiness } from "@swarmie/contracts";
import { nodeKey, sameNode, validNodeIdentity } from "./nodeScope";
import { WorkspaceStore, type Workspace } from "./workspace";

export type NodeRoute = {
  name: string;
  transport: { kind: string; url?: string; host?: string; socketPath?: string; path?: string };
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
export type { SshReadiness } from "@swarmie/contracts";
type Catalog = { nodes: ClientNode[]; selectedNodeId?: string };
export type NodesState = Catalog & { store: WorkspaceStore; error: string; loading: boolean };

// One state instance per retained node. Selection changes observation, never a chat's destination.
export class NodeClient {
  private local: WorkspaceStore;
  private stores = new Map<string, WorkspaceStore>();
  private listeners = new Set<() => void>();
  private state: NodesState;
  private timer?: ReturnType<typeof setInterval>;
  private observing = false;
  private disabled = new Set<string>();
  private selectionWrites = Promise.resolve();
  private token = "";
  private startPending?: Promise<void>;
  private requestId = 0;
  private running = false;
  private lifecycle = 0;
  private catalogLoaded = false;
  private catalogRetryAt = 0;
  private catalogPending?: Promise<void>;
  private catalogNeedsRefresh = true;
  private async control<T>(method: string, params: unknown = {}, timeout = 30_000): Promise<T> {
    const request = async (path: string, method: string, params: unknown, target?: RuntimeIdentity) => {
      const response = await clientRequest(path, { jsonrpc: "2.0", id: ++this.requestId, method, params, ...(target ? { expectedRuntime: target } : {}) }, this.token, path === "/client/rpc" ? timeout : 30_000);
      if (!response.ok) throw new Error(response.status === 401 ? "Enter the local connection token, then retry." : `Node management returned HTTP ${response.status}.`);
      const value = await response.json();
      if (value.error) throw new Error(value.error.message);
      return value.result;
    };
    const initialized = await request("/rpc", "runtime/initialize", {});
    const identity = initialized.runtimeIdentity as RuntimeIdentity;
    const local = this.local.getSnapshot().node;
    if (initialized.name !== "swarmie" || initialized.protocolVersion !== "1" || !validNodeIdentity(identity?.identity) ||
      !local || !sameNode(local, identity.identity)) throw new Error("The local connection now serves a different node. Saved work has been kept.");
    return request("/client/rpc", method, params, identity) as Promise<T>;
  }
  constructor(local = new WorkspaceStore()) {
    this.local = local;
    this.state = { nodes: [], store: local, error: "", loading: false };
    local.subscribe(() => this.publish({}));
  }
  getSnapshot = () => this.state;
  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };
  private publish(patch: Partial<NodesState>) {
    this.state = { ...this.state, ...patch };
    this.listeners.forEach((listener) => listener());
  }
  private store(node: ClientNode): WorkspaceStore {
    const key = nodeKey(node.identity);
    let store = this.stores.get(key);
    if (!store) {
      const local = this.local.getSnapshot().node;
      store = local && sameNode(local, node.identity) ? this.local
        : new WorkspaceStore({ node: node.identity, rpcPath: `/nodes/${encodeURIComponent(node.identity.environmentId)}/rpc` });
      this.stores.set(key, store);
      if (store !== this.local) store.subscribe(() => this.publish({}));
    }
    return store;
  }
  private startObservation() {
    if (nativeClient()) { void this.observeNative(); return; }
    if (this.timer || !this.running) return;
    this.timer = setInterval(() => {
      const lifecycle = this.lifecycle;
      if (!this.local.getSnapshot().connected) this.catalogNeedsRefresh = true;
      void this.local.poll().then(async () => {
        if (lifecycle !== this.lifecycle || !this.running || !this.local.getSnapshot().connected) return;
        if ((!this.catalogLoaded || this.catalogNeedsRefresh) && Date.now() >= this.catalogRetryAt)
          await this.loadCatalog();
      }).catch((error) => this.local.setError(error));
      for (const store of new Set(this.stores.values()))
        if (store !== this.local) void store.poll().catch((error) => store.setError(error));
    }, 500);
  }
  private async observeNative() {
    if (this.observing || !this.running) return;
    this.observing = true;
    const lifecycle = this.lifecycle;
    try {
      while (this.running && lifecycle === this.lifecycle) {
        const { events } = await observeClient();
        if (!this.running || lifecycle !== this.lifecycle) return;
        const stores = new Set([this.local, ...this.stores.values()]);
        for (const store of stores) {
          if (this.disabled.has(store.getSnapshot().node?.environmentId ?? "")) continue;
          const relevant = events.filter((event) => !event.environmentId || store.getSnapshot().node?.environmentId === event.environmentId);
          for (const event of relevant) {
            const connected = event.method === "client/connected" || (event.method === "client/status" && (event.params as { connected?: boolean })?.connected);
            const disconnected = event.method === "client/disconnected" || (event.method === "client/status" && !(event.params as { connected?: boolean })?.connected);
            if (disconnected && store.getSnapshot().connected) store.observeDisconnected();
            else if (connected && !store.getSnapshot().connected) await store.connect(this.token);
          }
          if (store.getSnapshot().connected) await store.poll();
        }
        if ((!this.catalogLoaded || this.catalogNeedsRefresh) && this.local.getSnapshot().connected) await this.loadCatalog();
      }
    } catch (error) { if (this.running && lifecycle === this.lifecycle) this.publish({ error: String(error) }); }
    finally { this.observing = false; if (this.running && lifecycle !== this.lifecycle) this.startObservation(); }
  }
  private loadCatalog() {
    if (this.catalogPending) return this.catalogPending;
    const pending = this.refresh(!this.catalogLoaded).finally(() => {
      if (this.catalogPending === pending) this.catalogPending = undefined;
    });
    this.catalogPending = pending;
    return pending;
  }
  async start(): Promise<void> {
    this.running = true;
    this.startObservation();
    if (this.startPending) return this.startPending;
    const lifecycle = this.lifecycle;
    const pending = (async () => {
      await this.local.connect(this.token);
      if (lifecycle !== this.lifecycle || !this.running || !this.local.getSnapshot().connected) return;
      await this.loadCatalog();
    })().finally(() => { if (this.startPending === pending) this.startPending = undefined; });
    this.startPending = pending;
    return pending;
  }
  async refresh(restoreSelection = false) {
    const lifecycle = this.lifecycle;
    this.publish({ loading: true, error: "" });
    try {
      const catalog = await this.control<Catalog>("nodes/list");
      if (lifecycle !== this.lifecycle) return;
      this.catalogLoaded = true;
      this.catalogNeedsRefresh = false;
      this.catalogRetryAt = 0;
      for (const node of catalog.nodes) {
        const store = this.store(node);
        if (!node.registered) store.disconnect();
      }
      const selected = restoreSelection ? catalog.nodes.find((node) => node.identity.environmentId === catalog.selectedNodeId) : undefined;
      const store = selected ? this.store(selected) : this.state.store;
      this.publish({ ...catalog, store });
      if (selected && selected.registered && store !== this.local)
        await store.connect(this.token);
    } catch (error) {
      if (lifecycle !== this.lifecycle) return;
      this.catalogRetryAt = Date.now() + 5000;
      this.publish({ error: error instanceof Error ? error.message : String(error) });
    } finally { if (lifecycle === this.lifecycle) this.publish({ loading: false }); }
  }
  async select(environmentId: string) {
    this.disabled.delete(environmentId);
    const lifecycle = this.lifecycle;
    const node = this.state.nodes.find((node) => node.identity.environmentId === environmentId);
    if (!node?.registered) throw new Error("This node is no longer registered. Add a connection to use it again.");
    const store = this.store(node);
    this.publish({ store, selectedNodeId: environmentId, error: "" });
    const save = this.selectionWrites.catch(() => {}).then(() => this.control<void>("nodes/select", { environmentId }));
    this.selectionWrites = save;
    try { await save; } catch (error) { if (lifecycle === this.lifecycle && this.state.store === store) this.publish({ error: String(error) }); }
    if (lifecycle !== this.lifecycle) return;
    if (!store.getSnapshot().connected) await store.connect(this.token);
  }
  async reconnect(token?: string) {
    const lifecycle = this.lifecycle;
    if (token !== undefined) this.token = token;
    await this.local.connect(this.token);
    if (lifecycle !== this.lifecycle || !this.local.getSnapshot().connected) return;
    await this.refresh(!this.catalogLoaded);
    if (lifecycle !== this.lifecycle) return;
    if (this.state.store !== this.local) await this.state.store.connect(this.token);
    this.startObservation();
  }
  localSnapshot = (): Workspace => this.local.getSnapshot();
  nodeSnapshot(node: ClientNode): Workspace { return this.store(node).getSnapshot(); }
  async probe(environmentId: string, harness: string) {
    const node = this.state.nodes.find((node) => node.identity.environmentId === environmentId);
    if (!node?.registered) throw new Error("This node has no active connection.");
    return this.store(node).probe(harness);
  }
  async pair(name: string, url: string, token: string) {
    await this.control("nodes/pair", { name, url, token });
    await this.refresh();
  }
  async inspectSsh(host: string, socketPath: string): Promise<SshReadiness> {
    return this.control<SshReadiness>("nodes/inspectSsh", { host, socketPath });
  }
  async prepareSsh(name: string, host: string, socketPath: string, action: "install" | "start"): Promise<SshReadiness> {
    const result = await this.control<SshReadiness>("nodes/prepareSsh", { name, host, socketPath, action }, 660_000);
    await this.refresh();
    return result;
  }
  async pairSsh(name: string, host: string, socketPath: string, runtimeIdentity: RuntimeIdentity) {
    await this.control("nodes/pairSsh", { name, host, socketPath, runtimeIdentity });
    await this.refresh();
  }
  async removeRoute(name: string, revision: number) {
    await this.control("nodes/removeRoute", { name, revision });
    await this.refresh();
  }
  async connect(environmentId: string) {
    this.disabled.delete(environmentId);
    const node = this.state.nodes.find((node) => node.identity.environmentId === environmentId);
    if (!node?.registered) throw new Error("Add a connection before reconnecting this node.");
    await this.store(node).connect(this.token);
  }
  disconnect(environmentId: string) {
    const node = this.state.nodes.find((node) => node.identity.environmentId === environmentId);
    if (node && !node.local) {
      this.disabled.add(environmentId);
      this.store(node).disconnect();
      if (nativeClient()) void this.control("nodes/disconnect", { environmentId }).catch((error) => this.publish({ error: String(error) }));
    }
  }
  status(node: ClientNode): string {
    const state = this.store(node).getSnapshot();
    return !node.registered ? "Removed" : state.connected ? "Connected" : state.busy ? "Connecting…" : state.error ? "Unavailable" : "Not connected";
  }
  error(node: ClientNode): string { return this.store(node).getSnapshot().error; }
  dispose() {
    this.running = false;
    this.lifecycle++;
    this.startPending = undefined;
    this.catalogPending = undefined;
    clearInterval(this.timer);
    this.timer = undefined;
    for (const store of new Set([this.local, ...this.stores.values()])) store.disconnect();
  }
}
