import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { WorkspaceStore } from "./workspace";
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
const session = (id = "native"): SessionSnapshot => ({
  id,
  state: "idle",
  options: { harness: "pi", cwd: "/project" },
  capabilities: { resume: true },
  sequence: 0,
});
it("persists a chosen project name when the folder is reopened or refreshed", async () => {
  const store = setup(false);
  vi.spyOn(store, "openProjectRecord").mockResolvedValue({ id: "/project", path: "/project", name: "project", worktrees: [] });
  expect(await store.openProject("/project", "Custom name")).toMatchObject({ name: "Custom name" });
  expect(await store.openProject("/project")).toMatchObject({ name: "Custom name" });
  await store.refreshProject("/project");
  expect(store.getSnapshot().projects[0]).toMatchObject({ name: "Custom name" });
});
it("names a Git project recovered from a native session when the descriptor has no display name", async () => {
  const store = setup(false);
  store.getSnapshot().projects = [];
  store.getSnapshot().git = true;
  vi.spyOn(store, "rpc").mockResolvedValue([session()]);
  vi.spyOn(store, "extension").mockResolvedValue({
    id: "/project", path: "/project", worktrees: [],
  });
  await store.restoreSessions();
  expect(store.getSnapshot().projects[0]).toMatchObject({
    id: "/project", name: "project", git: true,
  });
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
function setup(resources = true) {
  const store = new WorkspaceStore();
  store.getSnapshot().extensions = resources ? [{ id: "resources" }] : [];
  store.getSnapshot().projects = [
    {
      id: "/project",
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
      if (method !== "extension/invoke") throw new Error(`Unexpected ${method}`);
      if (params.command === "project.get") return defaults as any;
      throw new Error(`Unexpected ${params.command}`);
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
  resolve({ harness: "pi", model: "native-model" });
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
      { cwd: "/project", harness: "pi", model: "native-model", config: { thinkingLevel: "high" } },
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
    capabilities: { resume: true },
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
    capabilities: { resume: false },
  });
  vi.spyOn(store, "rpc").mockImplementation(async (method) => {
    if (method === "session/transcript") return [] as any;
    if (method === "session/queue/list")
      return { items: [], paused: false } as any;
    if (method === "session/get" || method === "session/resume")
      return { ...session("live"), capabilities: { resume: false } } as any;
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
      if (params.command === "project.get") {
        expect(params.arguments.cwd).toBe("/project");
        return {
          workspace: "/secondary",
          config: {
            approvalPolicy: "on-request",
            nested: { default: true, replace: "project" },
          },
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
    cwd: "/secondary",
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
    capabilities: { resume: true, images: true, rollback: true },
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
        return null as any;
      }
      if (method === "session/metadata") return null as any;
      if (method === "session/get" || method === "session/resume")
        return {
          ...session(),
          capabilities: { resume: true, images: true, rollback: true },
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
    capabilities: { metadata: { nativeSkillInputs: true } },
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
  const rpc = vi.spyOn(store, "rpc").mockResolvedValue(null);
  await store.send(id, "$same-name apply it");
  expect(rpc).toHaveBeenCalledExactlyOnceWith("session/prompt", {
    sessionId: "native",
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
    capabilities: { resume: true },
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
  const rpc = vi.spyOn(store, "rpc").mockResolvedValue(session());
  store.createThread();
  const id = store.active()!.id;
  store.updateThread(id, { draft: "A chat without a project", harness: "pi", model: "fixture/model" });
  const launched = store.ensureSession(id);
  expect(rpc).not.toHaveBeenCalled();
  resolve({ path: "/Documents" });
  await launched;
  expect(store.active()).toMatchObject({ projectId: "", worktreeId: "/Documents", draft: "A chat without a project", harness: "pi" });
  expect(store.getSnapshot().projects).toHaveLength(1);
  expect(invoke).toHaveBeenCalledWith("workspace", "conversation.directory", {});
  expect(rpc).toHaveBeenCalledWith("session/create", expect.objectContaining({ cwd: "/Documents", harness: "pi", model: "fixture/model" }));
});

it("persists a custom chat directory and resets to Documents without moving existing chats", async () => {
  const store = setup(false);
  const invoke = vi.spyOn(store, "invokeExtension").mockImplementation(async (_extension, _command, args: any) => ({ path: args.path || "/Documents" }) as any);
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
    if (method === "session/prompt") throw new Error("Prompt rejected");
    return {} as any;
  });
  store.createThread();
  const id = store.active()!.id;
  expect(hasConversation(store.active()!)).toBe(false);
  await expect(store.send(id, "First message")).rejects.toThrow("Prompt rejected");
  expect(hasConversation(store.active()!)).toBe(false);
  rpc.mockResolvedValue({} as any);
  await store.send(id, "First message");
  expect(hasConversation(store.active()!)).toBe(true);
  await store.flushPersistence();
  const saved = JSON.parse(vi.mocked(localStorage.setItem).mock.calls.at(-1)![1]);
  vi.spyOn(localStorage, "getItem").mockReturnValue(JSON.stringify(saved));
  const restored = new WorkspaceStore();
  await restored.ready();
  expect(hasConversation(restored.active()!)).toBe(true);
});

it("previews the effective native model without launching or listing a chat and pins it at send", async () => {
  const store = setup(false);
  store.getSnapshot().connected = true;
  store.createThread("/project");
  const id = store.active()!.id;
  const configuration = [{ id: "model", label: "Model", type: "select", value: "native-model", choices: [{ id: "native-model", label: "Native Model" }], live: true }];
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
  expect(rpc).toHaveBeenCalledWith("session/create", { harness: "codex", cwd: "/project", model: "native-model", config: {} });
});

it("discards a model preview returned after the draft changes harness", async () => {
  const store = setup(false);
  store.getSnapshot().connected = true;
  store.createThread("/project");
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
  store.updateThread(id, { sessionId: "native", config: { approvalPolicy: "never", sandbox: "danger-full-access" } });
  const patch = { approvalPolicy: "on-request", sandbox: "workspace-write", approvalsReviewer: "auto_review" };
  const rpc = vi.spyOn(store, "rpc").mockRejectedValue(new Error("Unsupported reviewer"));
  vi.spyOn(store, "loadControls").mockResolvedValue();
  await expect(store.configureMany(id, patch)).rejects.toThrow("Unsupported reviewer");
  expect(store.active()!.config).toEqual({ approvalPolicy: "never", sandbox: "danger-full-access" });
  rpc.mockResolvedValue(null);
  await store.configureMany(id, patch);
  expect(rpc).toHaveBeenLastCalledWith("agent/configure", { sessionId: "native", config: patch });
  expect(store.active()!.config).toEqual(patch);
});
