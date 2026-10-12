import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { WorkspaceStore } from "./workspace";
import { WorkspaceCache } from "./cache";
import type { RuntimeIdentity, SubmissionReceipt } from "@swarmie/contracts";

const identity: RuntimeIdentity = {
  identity: { environmentId: "node", storeId: "store", ownerScopeId: "owner" }, incarnationId: "runtime",
};
let requests: { method: string; params: any; expectedRuntime?: RuntimeIdentity }[];
let receipt: SubmissionReceipt;
let overrides: Record<string, { result?: any; error?: any }>;
beforeEach(() => {
  requests = [];
  overrides = {};
  receipt = { id: "intent", sessionId: "session", idempotencyKey: "original", turnId: "", delivery: "unknown", recoveryRequired: true, error: null, outcome: null };
  vi.stubGlobal("localStorage", { getItem: () => null, setItem: vi.fn() });
  vi.spyOn(WorkspaceCache.prototype, "load").mockResolvedValue(undefined);
  vi.spyOn(WorkspaceCache.prototype, "save").mockResolvedValue();
  vi.stubGlobal("window", { location: { search: "?client=native" }, addEventListener: vi.fn(), dispatchEvent: vi.fn(), zero: { invoke: async (_command: string, payload: any) => {
    const request = payload.request;
    requests.push(request);
    if (overrides[request.method]) return { response: { jsonrpc: "2.0", id: request.id, ...overrides[request.method] } };
    const result = request.method === "runtime/initialize" ? { name: "swarmie", protocolVersion: "1", runtimeIdentity: identity }
      : ["client/submissions", "client/sessionOpen"].includes(request.method) ? { activation: { status: receipt.recoveryRequired ? "blocked" : "attached" }, queue: { items: [], paused: false }, events: [], entries: [{ key: "original", receipt }], unresolved: receipt.recoveryRequired, snapshot: { id: "session", state: "idle", sequence: 0, turn: null } }
      : request.method === "settings/directory" ? { directory: { id: "directory", revision: 1 } }
      : ["directory/get", "client/chatDirectory"].includes(request.method) ? { path: "/work" } : [];
    return { response: { jsonrpc: "2.0", id: request.id, result } };
  } } });
});
afterEach(() => { vi.restoreAllMocks(); vi.unstubAllGlobals(); });

it("restores native uncertainty even when presentation submission keys have been removed", async () => {
  const store = new WorkspaceStore({ node: identity.identity });
  store.getSnapshot().chatDirectory = "/work";
  await store.connect();
  const id = store.active()!.id;
  store.updateThread(id, { sessionId: "session", state: "idle", draft: "Keep my draft", submissionKeys: [] });
  await store.recoverSubmissions(id);
  expect(store.active()).toMatchObject({ state: "needs_input", draft: "Keep my draft", submissionKeys: [{ key: "original", receipt }] });
  expect(requests.find((request) => request.method === "client/submissions")?.expectedRuntime).toEqual(identity);
  expect(requests.some((request) => ["session/prompt", "session/submission", "session/submission/reconcile"].includes(request.method))).toBe(false);
});

it("uses native confirmation to clear only the matching authored draft", async () => {
  const store = new WorkspaceStore({ node: identity.identity });
  store.getSnapshot().chatDirectory = "/work";
  await store.connect();
  const id = store.active()!.id;
  store.updateThread(id, {
    sessionId: "session", state: "needs_input", draft: "Original input",
    submissionKeys: [{ key: "original", recordId: "record" }],
    promptRecords: [{ id: "record", key: "fingerprint", afterSequence: 0, text: "Original input", attachments: [], contextItems: [] }],
  });
  receipt = { ...receipt, delivery: "accepted", recoveryRequired: false };
  await store.recoverSubmissions(id, true);
  expect(store.active()).toMatchObject({ draft: "", state: "idle", submitted: true });
  expect(requests.at(-1)?.params.reconcile).toBe(true);
  store.updateThread(id, { draft: "A new draft" });
  await store.recoverSubmissions(id);
  expect(store.active()?.draft).toBe("A new draft");
});

it("hydrates from one native projection without paging runtime history in the view", async () => {
  const store = new WorkspaceStore({ node: identity.identity });
  store.getSnapshot().chatDirectory = "/work";
  await store.connect();
  const id = store.active()!.id;
  store.updateThread(id, { sessionId: "session", state: "idle", draft: "Keep my draft", submissionKeys: [] });
  requests = [];
  await store.restoreThread(id);
  expect(requests.filter((request) => request.method === "client/sessionOpen")).toHaveLength(1);
  expect(requests.some((request) => ["session/transcript", "session/get", "client/submissions", "session/resume"].includes(request.method))).toBe(false);
  expect(store.active()).toMatchObject({ state: "needs_input", draft: "Keep my draft" });
});

it("submits launch intent in one native operation and uses its canonical directory", async () => {
  const store = new WorkspaceStore({ node: identity.identity });
  await store.connect();
  const id = store.active()!.id;
  store.updateThread(id, { harness: "mock", draft: "Keep my draft", worktreeId: "/view-only" });
  overrides["client/sessionCreate"] = { result: { id: "native", state: "idle", options: { cwd: "/canonical" }, capabilities: {} } };
  requests = [];
  await store.ensureSession(id);
  expect(requests.map(request => request.method)).toEqual(["client/sessionCreate"]);
  expect(requests[0].params).toMatchObject({ harness: "mock", config: {} });
  expect(requests[0].params).not.toHaveProperty("cwd");
  expect(store.active()).toMatchObject({ sessionId: "native", worktreeId: "/canonical", draft: "Keep my draft" });
});

it("applies native recovery controls after partial configuration without a second view request", async () => {
  const store = new WorkspaceStore({ node: identity.identity });
  await store.connect();
  const id = store.active()!.id;
  store.updateThread(id, { sessionId: "session", state: "idle", draft: "Keep my draft", config: { model: "old" }, model: "old" });
  overrides["client/sessionConfigure"] = { error: { code: -32001, message: "Partial configuration", data: {
    effectKnowledge: "unknown", controls: { configuration: [], models: [], values: { model: "observed" }, executionPolicy: {} },
  } } };
  requests = [];
  await expect(store.configureMany(id, { model: "selected" })).rejects.toThrow("Partial configuration");
  expect(requests.map(request => request.method)).toEqual(["client/sessionConfigure"]);
  expect(store.active()).toMatchObject({ model: "observed", config: { model: "observed" }, draft: "Keep my draft" });
});

it("renders the attached native queue without querying it from the view", async () => {
  const store = new WorkspaceStore({ node: identity.identity });
  await store.connect();
  const id = store.active()!.id;
  store.updateThread(id, { sessionId: "session", state: "idle", draft: "Keep my draft" });
  overrides["client/sessionOpen"] = { result: {
    events: [], entries: [], unresolved: false, activation: { status: "attached" },
    queue: { items: [{ id: "queued", createdAtMs: 1, prompt: { content: [{ type: "text", data: "Next input" }] } }], paused: true },
    snapshot: { id: "session", state: "idle", sequence: 0, turn: null, options: { harness: "mock", cwd: "/work" } },
  } };
  requests = [];
  await store.restoreThread(id);
  expect(requests.map(request => request.method)).toEqual(["client/sessionOpen"]);
  expect(store.active()).toMatchObject({ queue: [{ id: "queued" }], queuePaused: true, draft: "Keep my draft" });
});
