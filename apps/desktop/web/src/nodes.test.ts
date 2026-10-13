import "fake-indexeddb/auto";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { NodeClient, type ClientNode } from "./nodes";
const identity = (id: string) => ({ environmentId: id, storeId: `store-${id}`, ownerScopeId: `owner-${id}` });
const local: ClientNode = { identity: identity("local"), displayName: "This computer", local: true, registered: true, lastVerifiedAtMs: 0, connections: [] };
const remote: ClientNode = { ...local, identity: identity("remote"), displayName: "Remote", local: false, connections: [{ name: "route", transport: { kind: "http", url: "http://remote.invalid/rpc" }, authenticated: true, revision: 12 }] };
let catalog: ClientNode[], selected: string, offline: boolean, localOffline: boolean, clients: NodeClient[], persisted: Map<string, string>;
let requests: { path: string; method: string; params: any; expectedRuntime?: { identity: { environmentId: string } } }[];
beforeEach(() => {
  catalog = structuredClone([local, remote]); selected = "remote"; offline = false; localOffline = false; requests = []; clients = []; persisted = new Map();
  vi.stubGlobal("localStorage", { getItem: (key: string) => persisted.get(key) ?? null, setItem: (key: string, value: string) => persisted.set(key, value) });
  vi.stubGlobal("window", { addEventListener: vi.fn(), dispatchEvent: vi.fn() });
  vi.stubGlobal("fetch", vi.fn(async (path: string, options: RequestInit) => {
    const request = JSON.parse(options.body as string); requests.push({ path, ...request });
    if (path === "/rpc" && localOffline) throw new TypeError("local offline");
    if (path.startsWith("/nodes/") && offline) throw new TypeError("offline");
    const target = path.startsWith("/nodes/") ? remote : local;
    const result = request.method === "runtime/initialize" ? { name: "swarmie", protocolVersion: "1", runtimeIdentity: { identity: target.identity, incarnationId: "live" } }
      : request.method === "nodes/list" ? { nodes: catalog, selectedNodeId: selected }
      : request.method === "session/queue/list" ? { items: [], paused: false } : [];
    if (request.method === "nodes/select") selected = request.params.environmentId;
    if (request.method === "nodes/removeRoute") { catalog[1].connections = []; catalog[1].registered = false; }
    return new Response(JSON.stringify({ jsonrpc: "2.0", id: request.id, result }));
  }));
});
afterEach(() => { clients.forEach((client) => client.dispose()); vi.restoreAllMocks(); vi.unstubAllGlobals(); });
async function start() { const client = new NodeClient(); clients.push(client); await client.start(); return client; }
it("attaches locally first and reuses separate state across selection and multiple routes", async () => {
  const client = await start(); expect(requests[0]).toMatchObject({ path: "/rpc", method: "runtime/initialize" });
  const a = client.getSnapshot().store; a.updateThread(a.active()!.id, { draft: "Remote draft" });
  await client.select("local"); const b = client.getSnapshot().store; b.updateThread(b.active()!.id, { draft: "Local draft" });
  await client.select("remote"); expect(client.getSnapshot().store).toBe(a);
  expect(a.active()!.draft).toBe("Remote draft"); expect(b.active()!.draft).toBe("Local draft");
  catalog[1].connections.push({ ...remote.connections[0], name: "second", revision: 13 }); await client.refresh();
  expect(client.getSnapshot().store).toBe(a);
  expect(requests.filter((r) => r.method === "nodes/select").map((r) => r.params.environmentId)).toEqual(["local", "remote"]);
});
it("preserves unavailable-node drafts and never falls back to local execution", async () => {
  const client = await start(); const store = client.getSnapshot().store; store.updateThread(store.active()!.id, { draft: "Keep remote" });
  client.disconnect("remote"); offline = true; await client.select("remote");
  expect(client.getSnapshot().store).toBe(store); expect(store.active()!.draft).toBe("Keep remote");
  expect(store.getSnapshot().connected).toBe(false); expect(client.status(catalog[1])).toBe("Unavailable");
});
it("keeps local attachment active when another node is viewed", async () => {
  const client = await start(); await client.select("local"); const store = client.getSnapshot().store;
  client.disconnect("local"); requests = []; await client.select("remote"); await client.refresh();
  expect(store.getSnapshot().connected).toBe(true); expect(client.getSnapshot().store.getSnapshot().connected).toBe(true);
  expect(requests.filter((r) => r.path === "/rpc").every((r) => r.method === "runtime/initialize")).toBe(true);
  expect(requests.filter((r) => r.path === "/client/rpc").every((r) => r.expectedRuntime?.identity.environmentId === "local")).toBe(true);
});
it("guards removal with the observed revision and retains removed-node work", async () => {
  const client = await start(); const store = client.getSnapshot().store; store.updateThread(store.active()!.id, { draft: "Retained" });
  await client.removeRoute("route", 12);
  expect(requests.find((r) => r.method === "nodes/removeRoute")?.params).toEqual({ name: "route", revision: 12 });
  expect(client.getSnapshot().store).toBe(store); expect(store.active()!.draft).toBe("Retained"); expect(store.getSnapshot().connected).toBe(false);
  await expect(client.select("remote")).rejects.toThrow("no longer registered");
  expect(requests.some((r) => r.method === "runtime/shutdown" || r.method === "session/prompt")).toBe(false);
});
it("observes hidden stores and disposes only the client", async () => {
  const callbacks: (() => void)[] = [];
  vi.spyOn(globalThis, "setInterval").mockImplementation(((callback: () => void) => { callbacks.push(callback); return 100; }) as typeof setInterval);
  const clear = vi.spyOn(globalThis, "clearInterval"); const client = await start(); const a = client.getSnapshot().store;
  await client.select("local"); const b = client.getSnapshot().store;
  const ap = vi.spyOn(a, "poll").mockResolvedValue(undefined), bp = vi.spyOn(b, "poll").mockResolvedValue(undefined);
  callbacks[0](); expect(ap).toHaveBeenCalledOnce(); expect(bp).toHaveBeenCalledOnce();
  client.dispose(); expect(clear).toHaveBeenCalledWith(100);
  expect(requests.some((r) => r.method === "runtime/shutdown")).toBe(false); expect(a.getSnapshot().connected || b.getSnapshot().connected).toBe(false);
});
it("survives disposal and immediate restart during initial connection", async () => {
  const client = new NodeClient(); clients.push(client); const first = client.start(); client.dispose(); const second = client.start();
  await Promise.all([first, second]); expect(client.getSnapshot().nodes).toHaveLength(2); expect(client.getSnapshot().store.getSnapshot().connected).toBe(true);
});
it("sends remote credentials only to local administration and does not cache them", async () => {
  const client = await start(); await client.pair("new", "http://node.invalid", "private-secret");
  expect(requests.find((r) => r.method === "nodes/pair")).toMatchObject({ path: "/client/rpc", params: { token: "private-secret" } });
  await client.getSnapshot().store.flushPersistence(); expect([...persisted.values()].join()).not.toContain("private-secret");
  expect(requests.every((r) => r.path.startsWith("/"))).toBe(true);
});
it("marks gateway transport failure unavailable and retains delivery uncertainty", async () => {
  const client = await start(); const store = client.getSnapshot().store;
  store.updateThread(store.active()!.id, { draft: "Unconfirmed input" });
  vi.mocked(fetch).mockImplementationOnce(async (_path, options) => {
    const request = JSON.parse(options!.body as string);
    return new Response(JSON.stringify({ jsonrpc: "2.0", id: request.id, error: { code: -32006, message: "Remote acknowledgment lost", data: { effectKnowledge: "unknown" } } }));
  });
  await expect(store.rpc("session/prompt", { text: "once" })).rejects.toMatchObject({ code: -32006, data: { effectKnowledge: "unknown" } });
  expect(store.getSnapshot().connected).toBe(false); expect(store.active()!.draft).toBe("Unconfirmed input");
  expect(client.status(catalog[1])).toBe("Unavailable");
});

it("recovers from an initial local transport failure and loads the saved node selection", async () => {
  const callbacks: (() => void)[] = [];
  vi.spyOn(globalThis, "setInterval").mockImplementation(((callback: () => void) => { callbacks.push(callback); return 100; }) as typeof setInterval);
  localOffline = true;
  const client = await start(); expect(client.localSnapshot().connected).toBe(false); expect(client.getSnapshot().nodes).toHaveLength(0);
  localOffline = false; const now = Date.now(); vi.spyOn(Date, "now").mockReturnValue(now + 60_000);
  callbacks[0]();
  await vi.waitFor(() => expect(client.getSnapshot().store.getSnapshot().node?.environmentId).toBe("remote"));
  expect(client.localSnapshot().connected).toBe(true); expect(client.getSnapshot().nodes).toHaveLength(2);
  expect(requests.some((r) => r.method === "session/prompt")).toBe(false);
});
it("restores hidden local observation after transport loss without changing the viewed remote or resending input", async () => {
  const callbacks: (() => void)[] = [];
  vi.spyOn(globalThis, "setInterval").mockImplementation(((callback: () => void) => { callbacks.push(callback); return 100; }) as typeof setInterval);
  const client = await start(); const remoteStore = client.getSnapshot().store;
  await client.select("local"); const localStore = client.getSnapshot().store; await client.select("remote");
  localOffline = true; await expect(localStore.rpc("runtime/initialize")).rejects.toThrow();
  expect(client.localSnapshot().connected).toBe(false);
  localOffline = false; const now = Date.now(); vi.spyOn(Date, "now").mockReturnValue(now + 60_000); requests = [];
  callbacks[0]();
  await vi.waitFor(() => expect(client.localSnapshot().connected).toBe(true));
  await vi.waitFor(() => expect(requests.some((r) => r.method === "nodes/list")).toBe(true));
  expect(client.getSnapshot().store).toBe(remoteStore);
  expect(requests.some((r) => r.method === "session/prompt" || r.method === "runtime/shutdown")).toBe(false);
});

it("sends startup to native control with the saved default SSH route and revision", async () => {
  const client = await start();
  const target: ClientNode = { ...remote, connections: [
    ...remote.connections,
    { name: "custom", revision: 20, authenticated: false, transport: { kind: "ssh", host: "custom", socketPath: "/custom/socket" } },
    { name: "default", revision: 21, authenticated: false, transport: { kind: "ssh", host: "saved-alias", socketPath: "" } },
  ] };
  await client.startup(target, true, true);
  expect(requests.find((r) => r.method === "nodes/startup")).toMatchObject({ path: "/client/rpc", params: { environmentId: "remote", name: "default", revision: 21, enabled: true, boot: true } });
  expect(requests.find((r) => r.method === "nodes/startup")?.params).not.toHaveProperty("host");
});
