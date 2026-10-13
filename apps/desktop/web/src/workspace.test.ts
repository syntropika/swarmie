import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { WorkspaceStore, RpcError } from "./workspace";
import { WorkspaceCache } from "./cache";
import { hasConversation } from "./protocol";

const node = { environmentId: "view-node", storeId: "view-store", ownerScopeId: "view-owner" };
const session = (id = "session") => ({ id, state: "idle", options: { harness: "mock", cwd: "/project", config: {} }, capabilities: {}, sequence: 0 });
const projection = (snapshot = session(), extra = {}) => ({ snapshot, entries: [], unresolved: false, events: [], activation: { status: "attached" }, ...extra });
const policy = { requested: null, controlledSettings: [] };
beforeEach(() => {
  vi.stubGlobal("localStorage", { getItem: () => null, setItem: vi.fn() });
  vi.stubGlobal("window", { addEventListener: vi.fn(), dispatchEvent: vi.fn() });
  vi.spyOn(WorkspaceCache.prototype, "load").mockResolvedValue(undefined);
  vi.spyOn(WorkspaceCache.prototype, "save").mockResolvedValue();
});
afterEach(() => { vi.restoreAllMocks(); vi.unstubAllGlobals(); });
function setup() {
  const store = new WorkspaceStore({ node });
  store.getSnapshot().connected = true;
  store.getSnapshot().chatDirectory = "/project";
  store.createThread();
  const id = store.active()!.id;
  store.updateThread(id, { sessionId: "session", state: "idle", title: "Existing", harness: "mock" });
  const rpc = vi.spyOn(store, "rpc").mockImplementation(async method => {
    if (["client/sessionOpen", "client/sessionProjection", "client/sessionRecover", "client/submissions"].includes(method)) return projection() as any;
    if (method === "client/sessionQueue") return { items: [], paused: false } as any;
    if (method === "session/list") return [] as any;
    throw new Error(`Unexpected ${method}`);
  });
  return { store, id, rpc };
}
const accepted = (text: string, extra = {}) => ({
  input: { text, content: [{ type: "text", data: text }], attachments: [], contextItems: [], afterSequence: 0 },
  receipt: { id: "receipt", sessionId: "session", delivery: "accepted", recoveryRequired: false, turnId: "turn" }, ...extra,
});
it("uses the canonical project returned by one application request", async () => {
  const { store, rpc } = setup();
  rpc.mockImplementation(async method => method === "client/projectDefaults" ? { revision: 2, cwd: "/canonical" } as any : { id: "project", revision: 2, path: "/canonical", name: "Chosen name", worktrees: [] } as any);
  expect(await store.openProject("/selected", "Chosen name")).toMatchObject({ id: "project", name: "Chosen name" });
  expect(rpc).toHaveBeenCalledWith("client/projectOpen", { path: "/selected", name: "Chosen name" });
  await store.refreshProject("project");
  expect(rpc).toHaveBeenCalledWith("client/projectGet", { projectId: "project" });
  expect(rpc.mock.calls.some(([method]) => method === "directory/register" || method === "project/create")).toBe(false);
});
it("does not infer project membership from the path of an imported session", async () => {
  const { store, rpc } = setup();
  rpc.mockResolvedValue([session("imported")] as any);
  await store.restoreSessions();
  expect(store.getSnapshot().threads.find(t => t.sessionId === "imported")).toMatchObject({ projectId: "", worktreeId: "/project" });
});
it("waits for defaults while typing and deduplicates simultaneous session launches", async () => {
  const { store, rpc } = setup();
  store.getSnapshot().projects = [{ id: "project", name: "Project", revision: 1, path: "/project", worktrees: [{ id: "/project", path: "/project", branch: null, isMain: true, locked: false, prunable: false }] }];
  let resolve!: (value: unknown) => void;
  rpc.mockImplementation(async method => {
    if (method === "client/projectDefaults") return new Promise(r => { resolve = r; }) as any;
    if (method === "client/sessionCreate") return session("created") as any;
    throw new Error(`Unexpected ${method}`);
  });
  store.createThread("project"); const id = store.active()!.id;
  const first = store.ensureSession(id), second = store.ensureSession(id);
  store.updateThread(id, { draft: "Typed during defaults" });
  resolve({ revision: 1, cwd: "/project", harness: "mock", model: "selected" });
  await Promise.all([first, second]);
  expect(rpc.mock.calls.filter(([method]) => method === "client/sessionCreate")).toHaveLength(1);
  expect(store.active()).toMatchObject({ sessionId: "created", draft: "Typed during defaults", model: "selected" });
});
it("hydrates a closed conversation and dates without view-owned attachment decisions", async () => {
  const { store, id, rpc } = setup();
  rpc.mockResolvedValue(projection({ ...session(), state: "closed", sequence: 1, metadata: { createdAtMs: 10, updatedAtMs: 20 } } as any, { activation: { status: "inactive" }, events: [{ sessionId: "session", sequence: 1, timestampMs: 1000, type: "message.user", data: { content: [{ type: "text", data: "Saved" }] } }] }) as any);
  await store.restoreThread(id);
  expect(rpc).toHaveBeenCalledExactlyOnceWith("client/sessionOpen", { sessionId: "session" });
  expect(store.active()).toMatchObject({ state: "closed", createdAtMs: 10, updatedAtMs: 20 });
  expect(new Date(store.active()!.messages[0].createdAt!).getTime()).toBe(1000);
});
it("backfills message dates from one verified projection without resuming", async () => {
  const { store, id, rpc } = setup();
  store.updateThread(id, { messages: [{ id: "user-1", role: "user", content: "Saved" }] });
  rpc.mockResolvedValue(projection(session(), { events: [{ sessionId: "session", sequence: 1, type: "message.user", timestampMs: 1000 }] }) as any);
  await store.loadMessageTimes(id);
  expect(rpc).toHaveBeenCalledExactlyOnceWith("client/sessionProjection", { sessionId: "session" });
  expect(new Date(store.active()!.messages[0].createdAt!).getTime()).toBe(1000);
});
it("discards timestamp hydration after the displayed session changes", async () => {
  const { store, id, rpc } = setup(); let finish!: (value: unknown) => void;
  rpc.mockImplementationOnce(() => new Promise(r => { finish = r; }));
  const pending = store.loadMessageTimes(id);
  store.updateThread(id, { sessionId: "other", messages: [{ id: "user-1", role: "user", content: "Other" }] });
  finish(projection(session(), { events: [{ sessionId: "session", sequence: 1, type: "message.user", timestampMs: 1000 }] }));
  await pending; expect(store.active()!.messages[0].createdAt).toBeUndefined();
});
it("keeps composer and file edits made during an asynchronous projection", async () => {
  const { store, id, rpc } = setup(); let finish!: (value: unknown) => void;
  rpc.mockImplementationOnce(() => new Promise(r => { finish = r; }));
  const pending = store.restoreThread(id);
  store.updateThread(id, { draft: "New draft", fileDrafts: { "notes.txt": { original: "old", content: "edited" } } });
  finish(projection()); await pending;
  expect(store.active()).toMatchObject({ draft: "New draft", fileDrafts: { "notes.txt": { content: "edited" } } });
});
it("sends authored intent and lets the application interpret skills and attachments", async () => {
  const { store, id, rpc } = setup();
  const attachment = { id: "file", name: "document.pdf", mime: "application/pdf", image: false, binary: true, data: "ZmlsZQ==" };
  const context = { id: "skill", kind: "resource" as const, label: "Skill", text: "Selected", skill: { name: "selected", path: "/skills/selected" } };
  store.updateThread(id, { draft: "Original", attachments: [attachment], contextItems: [context] });
  rpc.mockImplementationOnce(async () => accepted("Original") as any);
  await store.send(id, "Original", [attachment]);
  expect(rpc).toHaveBeenCalledExactlyOnceWith("client/sessionInput", { sessionId: "session", text: "Original", attachments: [attachment], contextItems: [context], delivery: "prompt", idempotencyKey: expect.any(String) });
  expect(store.active()).toMatchObject({ draft: "", attachments: [], contextItems: [], submitted: true });
});
it("keeps a newer draft and early completion when input acknowledgment is delayed", async () => {
  const { store, id, rpc } = setup();
  store.updateThread(id, { draft: "Original" });
  rpc.mockImplementationOnce(async () => { store.updateThread(id, { draft: "New draft", state: "idle" }); return accepted("Original") as any; });
  await store.send(id, "Original");
  expect(store.active()).toMatchObject({ draft: "New draft", state: "idle", submitted: true });
});
it("retains a session and its draft when the application rejects input without effects", async () => {
  const { store, id, rpc } = setup(); store.updateThread(id, { draft: "Original" });
  rpc.mockRejectedValueOnce(new RpcError("Provider unavailable", -32001, { effectKnowledge: "none" }));
  await expect(store.send(id, "Original")).rejects.toThrow("Provider unavailable");
  expect(store.active()).toMatchObject({ sessionId: "session", state: "idle", draft: "Original", submissionKeys: [] });
  rpc.mockResolvedValueOnce(accepted("Original") as any); await store.send(id, "Original");
  expect(hasConversation(store.active()!)).toBe(true);
});
it("recovers lost confirmation with reads and keeps a newer draft", async () => {
  const { store, id, rpc } = setup();
  rpc.mockImplementation(async (method, params: any) => {
    if (method === "client/sessionInput") { store.updateThread(id, { draft: "New draft" }); throw new Error("Response lost"); }
    if (method === "client/submissions") {
      const key = store.active()!.submissionKeys![0].key;
      return projection(session(), { entries: [{ key, sessionId: "session", input: { ...accepted("Original").input }, receipt: { ...accepted("Original").receipt, idempotencyKey: key } }] }) as any;
    }
    throw new Error(`Unexpected ${method}`);
  });
  await expect(store.send(id, "Original")).rejects.toThrow("Response lost");
  expect(store.active()).toMatchObject({ state: "idle", draft: "New draft", submitted: true });
  expect(rpc.mock.calls.filter(([method]) => method === "client/sessionInput")).toHaveLength(1);
});
it("checks unresolved delivery through one application operation", async () => {
  const { store, id, rpc } = setup();
  rpc.mockResolvedValueOnce(projection(session(), { unresolved: true, activation: { status: "blocked" }, entries: [{ key: "original", receipt: { delivery: "unknown", recoveryRequired: true } }] }) as any);
  await store.checkSubmissionRecovery(id);
  expect(rpc).toHaveBeenCalledExactlyOnceWith("client/sessionRecover", { sessionId: "session" });
  expect(store.active()!.state).toBe("needs_input");
});
it("updates queue presentation with one operation and no cached prompt reconstruction", async () => {
  const { store, id, rpc } = setup();
  rpc.mockResolvedValueOnce({ items: [{ id: "queued", prompt: { content: [{ type: "text", data: "Updated" }] } }], paused: true } as any);
  await store.editQueue(id, "queued", "Updated");
  expect(rpc).toHaveBeenCalledExactlyOnceWith("client/sessionQueue", { sessionId: "session", id: "queued", action: "update", text: "Updated" });
  expect(store.active()).toMatchObject({ queuePaused: true, queue: [{ id: "queued" }] });
});
it("renders an edit draft and projection without running rollback or asset reads in the view", async () => {
  const { store, id, rpc } = setup();
  rpc.mockResolvedValueOnce({ projection: projection(), draft: { draft: "Original", attachments: [{ id: "image", data: "aW1hZ2U=" }], contextItems: [] } } as any);
  await store.editFrom(id, "user-2", "conversation_and_files");
  expect(rpc).toHaveBeenCalledExactlyOnceWith("client/sessionHistory", { sessionId: "session", action: "edit", messageId: "user-2", scope: "conversation_and_files" });
  expect(store.active()).toMatchObject({ draft: "Original", attachments: [{ id: "image" }] });
});
it("sends only a native tree anchor when creating an edit branch", async () => {
  const { store, id, rpc } = setup();
  rpc.mockResolvedValueOnce({ projection: projection(session("child")), draft: { draft: "Native original", attachments: [], contextItems: [] } } as any);
  const child = await store.editInBranch(id, { id: "native-anchor", text: "Untrusted view text" });
  expect(rpc).toHaveBeenCalledExactlyOnceWith("client/sessionHistory", { sessionId: "session", action: "branch", messageId: "native-anchor" });
  expect(store.active()).toMatchObject({ id: child, sessionId: "child", draft: "Native original" });
});
it("keeps draft and navigation unchanged after a failed native history operation", async () => {
  const { store, id, rpc } = setup(); store.updateThread(id, { draft: "Keep draft" });
  rpc.mockRejectedValueOnce(new RpcError("Saved attachment unavailable", -32001));
  await expect(store.editFrom(id, "user-2")).rejects.toThrow("Saved attachment unavailable");
  expect(rpc).toHaveBeenCalledTimes(1); expect(store.active()).toMatchObject({ id, draft: "Keep draft" });
});
it("does not apply an old event page after closing its session", async () => {
  const { store, id, rpc } = setup(); let finish!: (events: unknown) => void;
  rpc.mockImplementationOnce(() => new Promise(r => { finish = r; }));
  const pending = store.poll(); rpc.mockResolvedValueOnce(null);
  await store.close(id); finish([{ sessionId: "session", sequence: 1, type: "state.changed", data: "idle" }]);
  await pending; expect(store.active()!.state).toBe("closed");
});
it("does not turn an accepted completion into failure when optional review fails", async () => {
  const { store, id, rpc } = setup(); store.updateThread(id, { state: "running" });
  rpc.mockResolvedValueOnce([{ sessionId: "session", sequence: 1, type: "turn.ended", data: { outcome: "completed" } }] as any);
  vi.spyOn(store, "review").mockRejectedValue(new Error("Review unavailable"));
  await store.poll(); expect(store.active()!.state).toBe("idle"); expect(store.getSnapshot().error).toContain("Review unavailable");
});
it("discards a control preview returned after the draft changes harness", async () => {
  const { store, id, rpc } = setup(); store.updateThread(id, { sessionId: undefined, harness: "mock" }); let finish!: (value: unknown) => void;
  rpc.mockImplementationOnce(() => new Promise(r => { finish = r; }));
  const pending = store.previewControls(id); await vi.waitFor(() => expect(finish).toBeTypeOf("function"));
  store.updateThread(id, { harness: "other" }); finish({ capabilities: { images: { availability: "available" } } }); await pending;
  expect(store.active()!.capabilities).toBeUndefined();
});
it("applies observed configuration without another view-owned recovery call", async () => {
  const { store, id, rpc } = setup();
  rpc.mockRejectedValueOnce(new RpcError("Partial update", -32001, { controls: { configuration: [], models: [], values: { model: "observed" }, executionPolicy: policy } }));
  await expect(store.configureMany(id, { model: "selected" })).rejects.toThrow("Partial update");
  expect(store.active()).toMatchObject({ model: "observed", config: { model: "observed" } }); expect(rpc).toHaveBeenCalledTimes(1);
});
it("keeps invalid directory selections out of presentation settings", async () => {
  const { store, rpc } = setup(); rpc.mockRejectedValueOnce(new RpcError("Folder missing", -32001));
  await expect(store.setChatDirectory("/missing")).rejects.toThrow("Folder missing");
  expect(store.getSnapshot().chatDirectory).toBe("/project");
});
