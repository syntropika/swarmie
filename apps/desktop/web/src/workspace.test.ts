import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { WorkspaceStore, RpcError } from "./workspace";
import { WorkspaceCache } from "./cache";
import type {
  SessionSnapshot,
  Attachment,
  Event,
  PromptPart,
} from "./protocol";
import { hasConversation, reduceEvents } from "./protocol";

beforeEach(() => {
  vi.stubGlobal("localStorage", { getItem: () => null, setItem: vi.fn() });
  vi.stubGlobal("window", {
    addEventListener: vi.fn(),
    dispatchEvent: vi.fn(),
  });
  vi.spyOn(WorkspaceCache.prototype, "load").mockResolvedValue(undefined);
  vi.spyOn(WorkspaceCache.prototype, "save").mockResolvedValue();
});
afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});
const executionPolicy = {
  revision: 0, requested: null, status: "unknown" as const, planned: null, plannedProfile: null, observed: null,
  controlledSettings: [], choices: [], dispatchBlocked: false, reason: null, bindingId: null, nativeSession: null,
};
const session = (id = "native"): SessionSnapshot => ({
  id,
  state: "idle",
  options: { harness: "pi", cwd: "/project" },
  capabilities: { resume: { availability: "available" as const, source: "harness" as const, evidence: "declared" as const } },
  sequence: 0,
});
it("persists a chosen project name when the folder is reopened or refreshed", async () => {
  const store = setup(false);
  vi.spyOn(store, "openProjectRecord").mockResolvedValue({ id: "/project", revision: 1, path: "/project", name: "project", worktrees: [] });
  vi.spyOn(store, "rpc").mockImplementation(async (method) => {
    if (method === "directory/get") return { path: "/project" } as any;
    return { id: "/project", name: "Custom name", revision: 2, directory: { id: "directory", revision: 1 }, launch: {} } as any;
  });
  expect(await store.openProject("/project", "Custom name")).toMatchObject({ name: "Custom name" });
  expect(await store.openProject("/project")).toMatchObject({ name: "Custom name" });
  vi.mocked(store.openProjectRecord).mockResolvedValue({ id: "/project", revision: 2, path: "/project", name: "Custom name", worktrees: [] });
  await store.refreshProject("/project");
  expect(store.getSnapshot().projects[0]).toMatchObject({ name: "Custom name" });
});
it("does not infer project membership from a native session path", async () => {
  const store = setup(false);
  store.getSnapshot().projects = [];
  vi.spyOn(store, "rpc").mockResolvedValue([session()]);
  const open = vi.spyOn(store, "openProjectRecord");
  await store.restoreSessions();
  expect(open).not.toHaveBeenCalled();
  expect(store.getSnapshot().projects).toEqual([]);
  expect(store.getSnapshot().threads[0]).toMatchObject({ projectId: "", worktreeId: "/project" });
});
it("recovers the native public launch configuration for subsequent generation", async () => {
  const store = setup(false);
  const config = {
    executable: "/fixture/pi",
    provider: "fixture",
    thinkingLevel: "high",
  };
  vi.spyOn(store, "rpc").mockResolvedValue([
    {
      ...session(),
      options: {
        harness: "pi",
        cwd: "/project",
        model: "fixture/model",
        config,
      },
    },
  ]);
  await store.restoreSessions();
  expect(store.getSnapshot().threads[0]).toMatchObject({
    harness: "pi",
    model: "fixture/model",
    config,
  });
});
it("keeps native conversation dates when recovering and replaying historical threads", async () => {
  const store = setup(false);
  const saved = {
    ...session(),
    state: "closed",
    metadata: { createdAtMs: 1, updatedAtMs: 2 },
  };
  vi.spyOn(store, "rpc").mockImplementation(async (method) => {
    if (method === "session/list") return [saved] as any;
    if (method === "session/get") return saved as any;
    if (method === "session/transcript") return [] as any;
    throw new Error(`Unexpected ${method}`);
  });
  await store.restoreSessions();
  const id = store.getSnapshot().threads[0].id;
  expect(store.getSnapshot().threads[0]).toMatchObject({
    createdAtMs: 1,
    updatedAtMs: 2,
  });
  await store.restoreThread(id);
  expect(store.getSnapshot().threads[0]).toMatchObject({
    createdAtMs: 1,
    updatedAtMs: 2,
  });
});
const testNode = { environmentId: "workspace-test-node", storeId: "workspace-test-store", ownerScopeId: "workspace-test-owner" };
function setup(resources = true) {
  const store = new WorkspaceStore({ node: testNode });
  store.getSnapshot().extensions = resources ? [{ id: "resources" }] : [];
  store.getSnapshot().projects = [
    {
      id: "/project",
      revision: 1,
      path: "/project",
      name: "project",
      worktrees: [
        {
          id: "/project",
          path: "/project",
          branch: "main",
          isMain: true,
          locked: false,
          prunable: false,
        },
      ],
    },
  ];
  return store;
}
it("backfills saved message times with one read without resuming or rewriting the conversation", async () => {
  const store = setup(false);
  await store.ready();
  store.createThread();
  const id = store.active()!.id;
  store.getSnapshot().connected = true;
  const existing = new Date(3000);
  store.updateThread(id, { sessionId: "native", messages: [
    { id: "user-1", role: "user", content: [{ type: "text", text: "Original" }] },
    { id: "user-2", role: "user", content: [{ type: "text", text: "Second" }] },
    { id: "user-3", role: "user", createdAt: existing, content: [{ type: "text", text: "Keep date" }] },
  ] });
  const before = store.active()!.updatedAtMs;
  const rpc = vi.spyOn(store, "rpc").mockResolvedValue([1, 2, 3].map((sequence) => ({
    sessionId: "native", sequence, timestampMs: sequence * 1000, type: "message.user", data: {},
  })) as any);
  const first = store.loadMessageTimes(id);
  expect(store.loadMessageTimes(id)).toBe(first);
  await first;
  expect(rpc).toHaveBeenCalledExactlyOnceWith("session/transcript", { sessionId: "native", afterSequence: 0, limit: 256 });
  expect(store.active()!.messages.map((message) => message.createdAt)).toEqual([new Date(1000), new Date(2000), existing]);
  expect(store.active()!.messages[0].content).toEqual([{ type: "text", text: "Original" }]);
  expect(store.active()!.updatedAtMs).toBe(before);
});
it("does not apply transcript dates after switching the native session", async () => {
  const store = setup(false);
  await store.ready(); store.createThread();
  const id = store.active()!.id;
  store.getSnapshot().connected = true;
  store.updateThread(id, { sessionId: "old", messages: [{ id: "user-1", role: "user", content: [{ type: "text", text: "New session" }] }] });
  let resolve!: (value: any) => void;
  vi.spyOn(store, "rpc").mockImplementation(() => new Promise((done) => { resolve = done; }));
  const pending = store.loadMessageTimes(id);
  store.updateThread(id, { sessionId: "new" });
  resolve([{ sessionId: "old", sequence: 1, timestampMs: 1000, type: "message.user", data: {} }]);
  await pending;
  expect(store.active()!.messages[0].createdAt).toBeUndefined();
});
it("waits for project defaults despite typing and deduplicates simultaneous native launches", async () => {
  const store = setup();
  let resolve!: (value: unknown) => void;
  const defaults = new Promise((done) => {
    resolve = done;
  });
  const rpc = vi
    .spyOn(store, "rpc")
    .mockImplementation(async (method, params: any) => {
      if (method === "session/create") return session() as any;
      if (method === "project/get") return defaults as any;
      if (method === "directory/get") return { path: "/project" } as any;
      throw new Error(`Unexpected ${method}`);
    });
  store.createThread("/project");
  const id = store.active()!.id;
  store.updateThread(id, {
    draft: "Typed before defaults loaded",
    config: { thinkingLevel: "high" },
  });
  const first = store.ensureSession(id),
    second = store.ensureSession(id);
  expect(first).toBe(second);
  expect(
    rpc.mock.calls.filter(
      ([method]: any) => method === "session/create",
    ),
  ).toHaveLength(0);
  resolve({ revision: 1, directory: { id: "directory", revision: 1 }, launch: { harness: "pi", model: "native-model" } });
  const result = await first;
  expect(result.model).toBe("native-model");
  expect(result.draft).toBe("Typed before defaults loaded");
  expect(
    rpc.mock.calls.filter(
      ([method]: any) => method === "session/create",
    ),
  ).toEqual([
    [
      "session/create",
      { projectId: "/project", projectRevision: 1, harness: "pi", model: "native-model", config: { thinkingLevel: "high" } },
    ],
  ]);
});
it("resumes saved sessions through the resources extension", async () => {
  const store = setup(false);
  store.createThread("/project");
  store.getSnapshot().extensions = [{ id: "resources" }];
  const id = store.active()!.id;
  store.updateThread(id, {
    sessionId: "peer",
    state: "closed",
    capabilities: { resume: { availability: "available" as const, source: "harness" as const, evidence: "declared" as const } },
  });
  const rpc = vi.spyOn(store, "rpc").mockResolvedValue(session("peer"));
  await store.ensureSession(id);
  expect(rpc).toHaveBeenCalledExactlyOnceWith("extension/invoke", {
    extensionId: "resources",
    command: "session.resume",
    arguments: { sessionId: "peer" },
  });
});
it("recovers a still-live handle even when the native harness cannot resume closed sessions", async () => {
  const store = setup(false);
  store.createThread("/project");
  const id = store.active()!.id;
  store.updateThread(id, {
    sessionId: "live",
    state: "idle",
    capabilities: { resume: { availability: "unsupported" as const, source: "harness" as const, evidence: "declared" as const } },
  });
  vi.spyOn(store, "rpc").mockImplementation(async (method) => {
    if (method === "session/transcript") return [] as any;
    if (method === "session/queue/list")
      return { items: [], paused: false } as any;
    if (method === "session/get" || method === "session/resume")
      return { ...session("live"), capabilities: { resume: { availability: "unsupported" as const, source: "harness" as const, evidence: "declared" as const } } } as any;
    throw new Error(`Unexpected ${method}`);
  });
  await store.restoreThread(id);
  expect(store.active()!.state).toBe("idle");
  expect(store.active()!.queue).toEqual([]);
  expect(store.rpc).toHaveBeenCalledWith("session/resume", {
    sessionId: "live",
  });
});
it("launches a saved secondary workspace with project permissions and explicit nested overrides", async () => {
  const store = setup();
  store.getSnapshot().projects[0].worktrees.push({
    id: "/secondary",
    path: "/secondary",
    branch: "feature",
    isMain: false,
    locked: false,
    prunable: false,
  });
  const rpc = vi
    .spyOn(store, "rpc")
    .mockImplementation(async (method, params: any) => {
      if (method === "session/create") return session() as any;
      if (method === "directory/get") return { path: "/secondary" } as any;
      if (method === "project/get") {
        expect(params.projectId).toBe("/project");
        return {
          revision: 2, directory: { id: "secondary", revision: 1 },
          launch: { config: {
            approvalPolicy: "on-request",
            nested: { default: true, replace: "project" },
          } },
        } as any;
      }
      throw new Error(`Unexpected ${method}`);
    });
  store.createThread("/project");
  const id = store.active()!.id;
  store.updateThread(id, { config: { nested: { replace: "explicit" } } });
  await store.ensureSession(id);
  expect(rpc).toHaveBeenCalledWith("session/create", {
    harness: "codex",
    projectId: "/project",
    projectRevision: 2,
    model: null,
    config: {
      approvalPolicy: "on-request",
      nested: { default: true, replace: "explicit" },
    },
  });
});
it("restores original prompt, context, image, text and binary attachments before rewinding", async () => {
  const store = setup(false);
  store.createThread("/project");
  store.getSnapshot().extensions = [{ id: "workspace" }];
  const id = store.active()!.id;
  const attachments: Attachment[] = [
    {
      id: "pdf",
      name: "notes.pdf",
      mime: "application/pdf",
      binary: true,
      image: false,
      data: "AP+AUEZERg==",
    },
    {
      id: "image",
      name: "original.png",
      mime: "image/png",
      image: true,
      data: "AQID",
    },
    {
      id: "text",
      name: "literal.txt",
      mime: "text/plain",
      image: false,
      data: "Original attachment\nsecond line",
    },
  ];
  const context = [
    {
      id: "ctx",
      kind: "terminal" as const,
      label: "Build output",
      text: "actual terminal output",
      sourceId: "terminal-1",
    },
  ];
  store.updateThread(id, {
    sessionId: "native",
    state: "idle",
    capabilities: { resume: { availability: "available" as const, source: "harness" as const, evidence: "declared" as const }, images: { availability: "available" as const, source: "harness" as const, evidence: "declared" as const }, rollback: { availability: "available" as const, source: "harness" as const, evidence: "declared" as const } },
    draft: "Inspect originals",
    attachments,
    contextItems: context,
  });
  let events: Event[] = [];
  let unavailable = false;
  let workspaceOpened = false;
  const rpc = vi
    .spyOn(store, "rpc")
    .mockImplementation(async (method, params: any) => {
      if (method === "extension/invoke") {
        if (params.command === "project.open") {
          expect(params.arguments.path).toBe("/project");
          workspaceOpened = true;
          return { id: "/project", path: "/project" } as any;
        }
        if (params.command === "assets.write") {
          expect(workspaceOpened).toBe(true);
          return { path: "/private/notes.pdf", assetId: "retained-pdf" } as any;
        }
        if (params.command === "assets.read") {
          expect(params.arguments.assetId).toBe("retained-pdf");
          if (unavailable) throw new Error("Asset unavailable");
          return { data: attachments[0].data } as any;
        }
      }
      if (method === "session/prompt") {
        const content: PromptPart[] = params.content;
        events = [
          {
            sessionId: "native",
            sequence: 1,
            turnId: "turn",
            type: "turn.started",
            data: {},
          },
          {
            sessionId: "native",
            sequence: 2,
            turnId: "turn",
            type: "message.user",
            data: { content },
          },
          {
            sessionId: "native",
            sequence: 3,
            turnId: "turn",
            type: "turn.ended",
            data: {},
          },
        ];
        return { delivery: "accepted", recoveryRequired: false } as any;
      }
      if (method === "session/metadata") return null as any;
      if (method === "session/get" || method === "session/resume")
        return {
          ...session(),
          capabilities: { resume: { availability: "available" as const, source: "harness" as const, evidence: "declared" as const }, images: { availability: "available" as const, source: "harness" as const, evidence: "declared" as const }, rollback: { availability: "available" as const, source: "harness" as const, evidence: "declared" as const } },
        } as any;
      if (method === "session/transcript")
        return events.filter(
          (event) => event.sequence > params.afterSequence,
        ) as any;
      if (method === "session/queue/list")
        return { items: [], paused: false } as any;
      if (method === "session/rollback") {
        events.push({
          sessionId: "native",
          sequence: 4,
          turnId: "turn",
          type: "conversation.rolled_back",
          data: { turns: 1 },
        });
        return null as any;
      }
      throw new Error(`Unexpected ${method}`);
    });
  await store.send(id, "Inspect originals", attachments);
  await store.restoreThread(id);
  expect(store.active()!.promptRecords![0]).toMatchObject({
    text: "Inspect originals",
    nativeSequence: 2,
    attachments: [
      { data: "", assetId: "retained-pdf" },
      { data: "" },
      { data: "" },
    ],
  });
  unavailable = true;
  await expect(store.editFrom(id, "user-2")).rejects.toThrow(
    "Asset unavailable",
  );
  expect(rpc.mock.calls.some(([method]) => method === "session/rollback")).toBe(
    false,
  );
  unavailable = false;
  await store.editFrom(id, "user-2");
  expect(store.active()!.draft).toBe("Inspect originals");
  expect(store.active()!.contextItems).toEqual(context);
  expect(store.active()!.attachments).toEqual([
    { ...attachments[0], path: "/private/notes.pdf", assetId: "retained-pdf" },
    attachments[1],
    attachments[2],
  ]);
  expect(store.active()!.messages).toEqual([]);
  expect(rpc).toHaveBeenCalledWith("session/rollback", {
    sessionId: "native",
    turns: 1,
    scope: "conversation_only",
  });
});
it("submits an exact native skill path separately from prompt text and preserves it for edits", async () => {
  const store = setup(false);
  store.createThread("/project");
  const id = store.active()!.id;
  const skill = { name: "same-name", path: "/project/second/SKILL.md" };
  store.updateThread(id, {
    sessionId: "native",
    title: "Existing",
    state: "idle",
    capabilities: { nativeSkillInputs: { availability: "available", source: "harness", evidence: "declared" } },
    contextItems: [
      {
        id: "selected-skill",
        kind: "resource",
        label: "Skill: same-name",
        text: "Catalog path",
        path: skill.path,
        skill,
      },
    ],
  });
  const rpc = vi.spyOn(store, "rpc").mockResolvedValue({ delivery: "accepted", recoveryRequired: false });
  await store.send(id, "$same-name apply it");
  expect(rpc).toHaveBeenCalledExactlyOnceWith("session/prompt", {
    sessionId: "native",
    idempotencyKey: expect.any(String),
    content: [
      { type: "text", data: "$same-name apply it" },
      { type: "skill", data: skill },
    ],
  });
  expect(store.active()!.promptRecords![0].contextItems[0].skill).toEqual(
    skill,
  );
  expect(store.active()!.contextItems).toEqual([]);
});
it("keeps file and composer edits made during an asynchronous native transcript replay", async () => {
  const store = setup(false);
  store.createThread("/project");
  const id = store.active()!.id;
  store.updateThread(id, {
    sessionId: "native",
    title: "Before",
    draft: "Before",
    fileDrafts: { "hello.txt": { content: "Before", original: "Original" } },
  });
  let deliver!: (events: Event[]) => void;
  const page = new Promise<Event[]>((resolve) => {
    deliver = resolve;
  });
  vi.spyOn(store, "rpc").mockImplementation(async (method) => {
    if (method === "session/transcript") return page as any;
    if (method === "session/get" || method === "session/resume")
      return session() as any;
    if (method === "session/queue/list")
      return { items: [], paused: false } as any;
    throw new Error(`Unexpected ${method}`);
  });
  const restore = store.restoreThread(id);
  await Promise.resolve();
  store.updateThread(id, {
    title: "Renamed during replay",
    draft: "Typed during replay",
    fileDrafts: {
      "hello.txt": { content: "Edited during replay", original: "Original" },
    },
  });
  deliver([]);
  await restore;
  expect(store.active()).toMatchObject({
    title: "Renamed during replay",
    draft: "Typed during replay",
    fileDrafts: {
      "hello.txt": { content: "Edited during replay", original: "Original" },
    },
  });
});

function branchStore() {
  const store = setup(false);
  store.createThread("/project");
  const id = store.active()!.id;
  store.updateThread(id, {
    sessionId: "native",
    state: "idle",
    config: { thinkingLevel: "high" },
    capabilities: { resume: { availability: "available" as const, source: "harness" as const, evidence: "declared" as const } },
  });
  const rpc = vi
    .spyOn(store, "rpc")
    .mockImplementation(async (method, params: any) => {
      if (method === "session/fork") return session("branch") as any;
      if (method === "session/get" || method === "session/resume")
        return session(params.sessionId) as any;
      if (method === "session/transcript") return [] as any;
      if (method === "session/queue/list")
        return { items: [], paused: false } as any;
      throw new Error(`Unexpected ${method}`);
    });
  return { store, id, rpc };
}
it("selects the matching image when native tree prompts share the same text", async () => {
  const { store, id, rpc } = branchStore();
  const parts = (image: string): PromptPart[] => [
    { type: "text", data: "Describe image" },
    { type: "image", data: { data: image, mime_type: "image/png" } },
  ];
  const original = store.active()!;
  const replay = reduceEvents(original, [
    {
      sessionId: "native",
      sequence: 1,
      type: "message.user",
      data: { content: parts("AAAA") },
    },
    {
      sessionId: "native",
      sequence: 2,
      type: "message.user",
      data: { content: parts("BBBB") },
    },
  ]);
  store.updateThread(id, {
    messages: replay.messages,
    promptRecords: ["AAAA", "BBBB"].map((data, index) => ({
      id: `record-${index}`,
      key: `native-normalized-${index}`,
      afterSequence: 0,
      nativeSequence: index + 1,
      text: "Describe image",
      attachments: [
        {
          id: data,
          name: `${data}.png`,
          mime: "image/png",
          image: true,
          data: "",
        },
      ],
      contextItems: [],
    })),
  });
  const child = await store.editInBranch(id, {
    id: "native-entry-b",
    text: "Describe image",
    metadata: { prompt: { content: parts("BBBB") } },
  });
  expect(store.active()).toMatchObject({
    id: child,
    draft: "Describe image",
    config: { thinkingLevel: "high" },
  });
  expect(store.active()!.attachments).toEqual([
    {
      id: "BBBB",
      name: "BBBB.png",
      mime: "image/png",
      image: true,
      data: "BBBB",
    },
  ]);
  expect(rpc).toHaveBeenCalledWith("session/fork", {
    sessionId: "native",
    messageId: "native-entry-b",
    position: "before_user",
  });
});
it("restores imported native images and skill paths when no local prompt record exists", async () => {
  const { store, id } = branchStore();
  const skill = { name: "review", path: "/project/skill/SKILL.md" };
  await store.editInBranch(id, {
    id: "imported-user",
    text: "Imported image",
    metadata: {
      prompt: {
        content: [
          { type: "text", data: "Imported image" },
          { type: "image", data: { data: "AQID", mime_type: "image/png" } },
          { type: "skill", data: skill },
        ],
      },
    },
  });
  expect(store.active()!.attachments).toMatchObject([
    { name: "Restored image", data: "AQID", image: true },
  ]);
  expect(store.active()!.contextItems?.[0].skill).toEqual(skill);
  expect(store.active()!.draft).toBe("Imported image");
});
it("does not fork when recovering a retained binary attachment fails", async () => {
  const { store, id, rpc } = branchStore();
  store.updateThread(id, {
    promptRecords: [
      {
        id: "saved",
        key: "legacy",
        afterSequence: 0,
        nativeSequence: 2,
        text: "Original",
        contextItems: [],
        attachments: [
          {
            id: "pdf",
            name: "original.pdf",
            mime: "application/pdf",
            image: false,
            binary: true,
            data: "",
            assetId: "missing",
          },
        ],
      },
    ],
  });
  await expect(
    store.editInBranch(id, { id: "user-2", text: "Original" }),
  ).rejects.toThrow("Unexpected extension/invoke");
  expect(rpc.mock.calls.some(([method]) => method === "session/fork")).toBe(
    false,
  );
  expect(store.active()!.id).toBe(id);
});
it("does not guess between identical native prompts with different original context", async () => {
  const { store, id, rpc } = branchStore();
  const original = store.active()!;
  const replay = reduceEvents(
    original,
    [1, 2].map((sequence) => ({
      sessionId: "native",
      sequence,
      type: "message.user",
      data: { content: [{ type: "text", data: "Identical" }] },
    })),
  );
  store.updateThread(id, {
    messages: replay.messages,
    promptRecords: [1, 2].map((nativeSequence) => ({
      id: `saved-${nativeSequence}`,
      key: "legacy",
      afterSequence: 0,
      nativeSequence,
      text: "Identical",
      attachments: [],
      contextItems: [
        {
          id: `ctx-${nativeSequence}`,
          kind: "terminal",
          label: `Output ${nativeSequence}`,
          text: "Selected original context",
        },
      ],
    })),
  });
  await expect(
    store.editInBranch(id, { id: "native-entry", text: "Identical" }),
  ).rejects.toThrow("Several original prompts");
  expect(rpc.mock.calls.some(([method]) => method === "session/fork")).toBe(
    false,
  );
});

it("launches a projectless chat in its resolved directory without inheriting a project", async () => {
  const store = setup();
  store.getSnapshot().connected = true;
  let resolve!: (value: { path: string }) => void;
  const directory = new Promise<{ path: string }>((done) => { resolve = done; });
  const invoke = vi.spyOn(store, "invokeExtension").mockImplementation(async (_extension, command) => {
    if (command === "conversation.directory") return directory as any;
    throw new Error(`Unexpected ${command}`);
  });
  const rpc = vi.spyOn(store, "rpc").mockImplementation(async (method) => {
    if (method === "settings/directory") return null as any;
    if (method === "directory/register") return { id: "documents", revision: 1, path: "/Documents" } as any;
    if (method === "settings/setDirectory") return {} as any;
    if (method === "session/create") return { ...session(), options: { harness: "pi", cwd: "/Documents" } } as any;
    throw new Error(`Unexpected ${method}`);
  });
  store.createThread();
  const id = store.active()!.id;
  store.updateThread(id, { draft: "A chat without a project", harness: "pi", model: "fixture/model" });
  const launched = store.ensureSession(id);
  expect(rpc.mock.calls.some(([method]) => method === "session/create")).toBe(false);
  resolve({ path: "/Documents" });
  await launched;
  expect(store.active()).toMatchObject({ projectId: "", worktreeId: "/Documents", draft: "A chat without a project", harness: "pi" });
  expect(store.getSnapshot().projects).toHaveLength(1);
  expect(invoke).toHaveBeenCalledWith("workspace", "conversation.directory", {});
  expect(rpc).toHaveBeenCalledWith("session/create", { harness: "pi", model: "fixture/model", config: {} });
});

it("persists a custom chat directory and resets to Documents without moving existing chats", async () => {
  const store = setup(false);
  const invoke = vi.spyOn(store, "invokeExtension").mockImplementation(async (_extension, _command, args: any) => ({ path: args.path || "/Documents" }) as any);
  vi.spyOn(store, "rpc").mockImplementation(async (method, params: any) => {
    if (method === "directory/register") return { id: params.path, revision: 1, path: params.path } as any;
    if (method === "settings/directory") return null as any;
    if (method === "settings/setDirectory") return {} as any;
    throw new Error(`Unexpected ${method}`);
  });
  await store.setChatDirectory(" /custom ");
  store.createThread();
  const first = store.active()!;
  expect(first).toMatchObject({ projectId: "", worktreeId: "/custom" });
  expect(JSON.parse(vi.mocked(localStorage.setItem).mock.calls.at(-1)![1])).toMatchObject({ customChatDirectory: "/custom" });
  await store.setChatDirectory();
  expect(invoke).toHaveBeenLastCalledWith("workspace", "conversation.directory", {});
  store.createThread();
  expect(store.active()).toMatchObject({ projectId: "", worktreeId: "/Documents" });
  expect(store.getSnapshot().threads.find((thread) => thread.id === first.id)?.worktreeId).toBe("/custom");
  expect(store.getSnapshot().customChatDirectory).toBeUndefined();
});

it("keeps a rejected chat directory out of settings and native launches", async () => {
  const store = setup(false);
  store.getSnapshot().chatDirectory = "/Documents";
  vi.spyOn(store, "invokeExtension").mockRejectedValue(new Error("Folder does not exist"));
  await expect(store.setChatDirectory("/missing")).rejects.toThrow("Folder does not exist");
  expect(store.getSnapshot().chatDirectory).toBe("/Documents");
  expect(store.getSnapshot().customChatDirectory).toBeUndefined();
});

it("recovers a native chat in the default directory outside project groups", async () => {
  const store = setup(false);
  store.getSnapshot().chatDirectory = "/Documents";
  vi.spyOn(store, "rpc").mockResolvedValue([{ ...session(), options: { harness: "pi", cwd: "/Documents" } }]);
  const open = vi.spyOn(store, "openProjectRecord");
  await store.restoreSessions();
  expect(open).not.toHaveBeenCalled();
  expect(store.getSnapshot().threads[0]).toMatchObject({ projectId: "", worktreeId: "/Documents" });
});

it("shows a conversation only after the first prompt is accepted and keeps it visible after reload", async () => {
  const store = setup(false);
  store.getSnapshot().chatDirectory = "/Documents";
  const rpc = vi.spyOn(store, "rpc").mockImplementation(async (method) => {
    if (method === "session/create") return session() as any;
    if (method === "session/prompt") throw new RpcError("Prompt rejected", -32002);
    return {} as any;
  });
  store.createThread();
  const id = store.active()!.id;
  expect(hasConversation(store.active()!)).toBe(false);
  await expect(store.send(id, "First message")).rejects.toThrow("Prompt rejected");
  expect(hasConversation(store.active()!)).toBe(false);
  rpc.mockResolvedValue({ delivery: "accepted", recoveryRequired: false } as any);
  await store.send(id, "First message");
  expect(hasConversation(store.active()!)).toBe(true);
  await store.flushPersistence();
  const saved = JSON.parse(vi.mocked(localStorage.setItem).mock.calls.at(-1)![1]);
  vi.spyOn(localStorage, "getItem").mockReturnValue(JSON.stringify(saved));
  const restored = new WorkspaceStore({ node: testNode });
  await restored.ready();
  expect(hasConversation(restored.active()!)).toBe(true);
});

it("previews the effective native model without launching or listing a chat and pins it at send", async () => {
  const store = setup(false);
  store.createThread("/project");
  store.getSnapshot().connected = true;
  const id = store.active()!.id;
  const configuration = [{ id: "model", label: "Model", type: "select" as const, value: "native-model", choices: [{ id: "native-model", label: "Native Model" }], application: "next_turn" as const }];
  const rpc = vi.spyOn(store, "rpc").mockImplementation(async (method) => {
    if (method === "runtime/configuration") return { configuration, models: configuration[0].choices } as any;
    if (method === "session/create") return session() as any;
    throw new Error(`Unexpected ${method}`);
  });
  await Promise.all([store.previewControls(id), store.previewControls(id)]);
  expect(rpc.mock.calls.filter(([method]) => method === "runtime/configuration")).toHaveLength(1);
  expect(store.active()!.sessionId).toBeUndefined();
  expect(hasConversation(store.active()!)).toBe(false);
  expect(store.active()!.configurations?.[0].value).toBe("native-model");
  await store.ensureSession(id);
  expect(rpc).toHaveBeenCalledWith("session/create", { harness: "codex", projectId: "/project", projectRevision: 1, model: "native-model", config: {} });
});

it("discards a model preview returned after the draft changes harness", async () => {
  const store = setup(false);
  store.createThread("/project");
  store.getSnapshot().connected = true;
  let resolve!: (value: any) => void;
  vi.spyOn(store, "rpc").mockImplementation(() => new Promise((done) => { resolve = done; }));
  const id = store.active()!.id;
  const pending = store.previewControls(id);
  store.updateThread(id, { harness: "claude" });
  resolve({ configuration: [{ id: "model", value: "codex-model" }], models: [{ id: "codex-model", label: "Codex Model" }] });
  await pending;
  expect(store.active()!.models).toBeUndefined();
  expect(store.active()!.configurations).toBeUndefined();
});

it("applies permission settings atomically and retains the current config when native configuration fails", async () => {
  const store = setup(false);
  store.createThread();
  const id = store.active()!.id;
  store.updateThread(id, { sessionId: "native", config: { executionProfile: "full", approvalPolicy: "never", sandbox: "danger-full-access" } });
  const patch = { approvalPolicy: "on-request", sandbox: "workspace-write", approvalsReviewer: "auto_review" };
  const rpc = vi.spyOn(store, "rpc").mockRejectedValue(new Error("Unsupported reviewer"));
  vi.spyOn(store, "loadControls").mockResolvedValue();
  await expect(store.configureMany(id, patch)).rejects.toThrow("Unsupported reviewer");
  expect(store.active()!.config).toEqual({ executionProfile: "full", approvalPolicy: "never", sandbox: "danger-full-access" });
  rpc.mockResolvedValue({ values: patch, configuration: [], executionPolicy });
  await store.configureMany(id, patch);
  expect(rpc).toHaveBeenLastCalledWith("agent/configure", { sessionId: "native", config: patch });
  expect(store.active()!.config).toEqual(patch);
});

it("uses confirmed configuration values when a model change resets other settings", async () => {
  const store = setup(false);
  store.createThread();
  const id = store.active()!.id;
  store.updateThread(id, { sessionId: "native", model: "previous", config: { model: "previous", effort: "high", fast: true } });
  vi.spyOn(store, "rpc").mockResolvedValue({
    executionPolicy,
    values: { model: "selected", effort: "low", fast: false },
    configuration: [{ id: "effort", label: "Thinking", type: "select", application: "next_turn", value: "low" }],
  });
  await store.configureMany(id, { model: "selected" });
  expect(store.active()!.model).toBe("selected");
  expect(store.active()!.config).toEqual({ model: "selected", effort: "low", fast: false });
  expect(store.active()!.configurations?.[0].value).toBe("low");
});

it("persists submission identity before dispatch and recovers a lost response with reads", async () => {
  const store = setup(false);
  store.createThread();
  const id = store.active()!.id;
  store.updateThread(id, { sessionId: "native", worktreeId: "/project", draft: "Original" });
  let originalKey = "";
  const rpc = vi.spyOn(store, "rpc").mockImplementation(async (method, params: any) => {
    if (method === "session/prompt") {
      originalKey = params.idempotencyKey;
      const saved = JSON.parse(vi.mocked(localStorage.setItem).mock.calls.at(-1)![1]);
      expect(saved.threads[0].submissionKeys[0].key).toBe(originalKey);
      store.updateThread(id, { draft: "New draft" });
      throw new Error("Response lost");
    }
    if (method === "session/submission") {
      expect(params.idempotencyKey).toBe(originalKey);
      return { id: "submission", sessionId: "native", idempotencyKey: originalKey, turnId: "turn", delivery: "accepted", recoveryRequired: false, outcome: "completed", error: null } as any;
    }
    if (method === "session/get") return session() as any;
    throw new Error(`Unexpected ${method}`);
  });
  await expect(store.send(id, "Original")).rejects.toThrow("Response lost");
  expect(store.active()).toMatchObject({ state: "idle", draft: "New draft", submitted: true });
  expect(rpc.mock.calls.filter(([method]) => method === "session/prompt")).toHaveLength(1);
  expect(rpc.mock.calls.some(([method]) => method === "session/resume")).toBe(false);
});

it("keeps uncertain delivery blocked with its original key and draft", async () => {
  const store = setup(false);
  store.createThread();
  const id = store.active()!.id;
  store.updateThread(id, { sessionId: "native", worktreeId: "/project", draft: "Original" });
  const rpc = vi.spyOn(store, "rpc").mockImplementation(async (method, params: any) => {
    if (method === "session/prompt") throw new Error("Disconnected");
    if (method === "session/submission") return { delivery: "unknown", recoveryRequired: true, idempotencyKey: params.idempotencyKey } as any;
    throw new Error(`Unexpected ${method}`);
  });
  await expect(store.send(id, "Original")).rejects.toThrow("Disconnected");
  expect(store.active()).toMatchObject({ state: "needs_input", draft: "Original" });
  expect(store.active()!.submissionKeys).toHaveLength(1);
  expect(hasConversation(store.active()!)).toBe(true);
  await expect(store.send(id, "Original")).rejects.toThrow("Choose Queue follow-up");
  expect(rpc.mock.calls.filter(([method]) => method === "session/prompt")).toHaveLength(1);
});

it("does not dispatch when local storage cannot retain the submission key", async () => {
  const store = setup(false);
  store.createThread();
  const id = store.active()!.id;
  store.updateThread(id, { sessionId: "native", worktreeId: "/project" });
  const rpc = vi.spyOn(store, "rpc");
  vi.mocked(localStorage.setItem).mockImplementation(() => { throw new Error("Storage full"); });
  await expect(store.send(id, "Original")).rejects.toThrow("Storage full");
  expect(rpc).not.toHaveBeenCalled();
  expect(store.active()!.state).toBe("idle");
  expect(store.active()!.submissionKeys).toEqual([]);
});

it("checks native submission evidence without resending and preserves an edited draft", async () => {
  const store = setup(false);
  store.createThread();
  const id = store.active()!.id;
  store.updateThread(id, { sessionId: "native", draft: "Edited draft", submissionKeys: [{key:"original",recordId:"record",receipt:{id:"attempt",sessionId:"native",idempotencyKey:"original",turnId:"turn",delivery:"unknown",recoveryRequired:true,outcome:null,error:null}}] });
  const rpc = vi.spyOn(store, "rpc").mockImplementation(async (method, params: any) => {
    if (method === "session/submission/reconcile") {
      expect(params.idempotencyKey).toBe("original");
      return {id:"attempt",sessionId:"native",idempotencyKey:"original",turnId:"turn",delivery:"accepted",recoveryRequired:false,outcome:"completed",error:null} as any;
    }
    if (method === "session/get" || method === "session/resume") return { id:"native",state:"idle",sequence:3,capabilities:{},metadata:{},options:{harness:"codex"} } as any;
    if (method === "session/transcript") return [] as any;
    if (method === "session/queue/list") return {items:[],paused:true} as any;
    throw new Error(`Unexpected ${method}`);
  });
  await store.checkSubmissionRecovery(id);
  expect(store.active()).toMatchObject({draft:"Edited draft",state:"idle"});
  expect(store.active()!.submissionKeys![0].receipt).toMatchObject({outcome:"completed",recoveryRequired:false});
  expect(rpc.mock.calls.some(([method]) => method === "session/prompt")).toBe(false);
});

it("keeps unsupported native recovery visible and paused", async () => {
  const store = setup(false);
  store.createThread();
  const id = store.active()!.id;
  store.updateThread(id, { sessionId:"native",draft:"Original",submissionKeys:[{key:"original",recordId:"record"}] });
  const rpc = vi.spyOn(store,"rpc").mockImplementation(async (method) => {
    if (method === "session/resume") return {} as any;
    if (method === "session/submission/reconcile") throw new RpcError("unsupported", -32001);
    throw new Error(`Unexpected ${method}`);
  });
  await store.checkSubmissionRecovery(id);
  expect(store.active()).toMatchObject({state:"needs_input",draft:"Original"});
  expect(store.active()!.error).toContain("cannot confirm the original submission");
  expect(rpc.mock.calls.some(([method]) => method === "session/prompt")).toBe(false);
});

it("clears a draft common selection only when an advertised native policy control is explicitly changed", async () => {
  const store = setup(false); store.createThread();
  const id = store.active()!.id;
  store.updateThread(id, { config: { executionProfile: "ask" }, executionPolicy: { ...executionPolicy, controlledSettings: ["nativeApproval"] } });
  await store.configureMany(id, { model: "selected" });
  expect(store.active()!.config?.executionProfile).toBe("ask");
  await store.configureMany(id, { nativeApproval: "custom" });
  expect(store.active()!.config).toEqual({ model: "selected", nativeApproval: "custom" });
  await store.configureMany(id, { executionProfile: "review" });
  expect(store.active()!.config?.executionProfile).toBe("review");
});
