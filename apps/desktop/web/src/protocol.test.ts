import { describe, expect, it, vi, beforeEach } from "vitest";
import { newThread, reduceEvents, type Event, type Thread } from "./protocol";
import { WorkspaceStore, RpcError } from "./workspace";
const testNode = { environmentId: "e", storeId: "s", ownerScopeId: "o" };
const event = (
  sequence: number,
  type: string,
  data: unknown,
  sessionId = "s",
): Event => ({ sessionId, sequence, type, data, turnId: "turn" });

describe("ordered provider replay", () => {
  it("records the actual rerouted model without replacing the requested selection", () => {
    const initial = {...newThread("p","w","codex"),sessionId:"s",model:"requested-model"};
    const rerouted = reduceEvents(initial,[event(1,"turn.started",{}),event(2,"model.rerouted",{fromModel:"requested-model",toModel:"actual-model",reason:"fixture"}),event(3,"turn.ended",{outcome:"completed"})]);
    expect(rerouted.model).toBe("requested-model");
    expect(rerouted.actualModel).toBe("actual-model");
    expect(reduceEvents(rerouted,[event(4,"turn.started",{})]).actualModel).toBeUndefined();
  });
  it("keeps sparse tool identity and input, appends deltas, and retains output on denial", () => {
    const initial = { ...newThread("p", "w", "codex"), sessionId: "s" };
    const updated = reduceEvents(initial, [
      event(1, "tool.started", {id:"tool",name:"Bash",status:"running",input:{command:"fixture"},metadata:{kind:"command",locations:["hello.txt"]}}),
      event(2, "tool.updated", {id:"tool",name:null,status:"running",input:null,output:"first",metadata:{kind:null,outputDelta:"first"}}),
      event(3, "tool.updated", {id:"tool",name:null,status:"running",input:null,output:" second",metadata:{outputDelta:" second",progress:"working"}}),
      event(4, "tool.completed", {id:"tool",name:null,status:"denied",input:null,output:null,metadata:{outputDelta:null}}),
    ]);
    expect(updated.tools[0]).toMatchObject({name:"Bash",input:{command:"fixture"},output:"first second",status:"denied",metadata:{kind:"command",locations:["hello.txt"],progress:"working"}});
    expect(initial.tools).toEqual([]);
    expect(reduceEvents(updated,[event(4,"tool.completed",{id:"tool",output:"duplicate"})]).tools[0].output).toBe("first second");
  });
  it("preserves the provider's authentication error instead of displaying null", () => {
    const thread = { ...newThread("p", "w", "claude"), sessionId: "s" };
    const message =
      "Failed to authenticate: OAuth session expired and could not be refreshed";
    expect(
      reduceEvents(thread, [
        { ...event(1, "error", { message: "null" }), raw: { result: message } },
      ]).error,
    ).toBe(message);
    expect(
      reduceEvents(thread, [event(1, "error", { message: null })]).error,
    ).toContain("Check its authentication");
  });
  it("deduplicates pages, ignores other sessions and preserves separate native text items", () => {
    const initial = { ...newThread("p", "w", "codex"), sessionId: "s" };
    const first = [
      event(1, "message.user", { content: [{ type: "text", data: "Hello" }] }),
      event(2, "text.delta", { text: "Reading.", item_id: "commentary" }),
    ];
    const once = reduceEvents(initial, first);
    const twice = reduceEvents(once, [
      ...first,
      event(99, "text.delta", { text: "Wrong thread" }, "other"),
      event(3, "text.delta", { text: "Done.", item_id: "answer" }),
    ]);
    expect(twice.sequence).toBe(3);
    expect(twice.messages).toHaveLength(2);
    expect(twice.messages[1].content).toEqual([
      { type: "text", text: "Reading.\n\nDone." },
    ]);
    expect(initial.messages).toHaveLength(0);
  });
  it("retains concurrent permission requests and clears only the answered request", () => {
    const initial = { ...newThread("p", "w", "codex"), sessionId: "s" };
    const pending = reduceEvents(initial, [
      event(1, "permission.requested", {
        id: "a",
        title: "First",
        options: [],
      }),
      event(2, "permission.requested", {
        id: "b",
        title: "Second",
        options: [],
      }),
      event(3, "permission.resolved", { id: "a" }),
    ]);
    expect(pending.permissions.map((p) => p.id)).toEqual(["b"]);
    const finished = reduceEvents(pending, [
      event(4, "state.changed", "running"),
      event(5, "turn.ended", { outcome: "completed" }),
    ]);
    expect(finished.state).toBe("idle");
    expect(finished.permissions).toEqual([]);
  });
});

describe("session submission", () => {
  beforeEach(() => {
    vi.stubGlobal("localStorage", { getItem: () => null, setItem: vi.fn() });
  });
  it("keeps the accepted session available for retry after a rejected prompt", async () => {
    const workspace = new WorkspaceStore({ node: testNode });
    const rpc = vi.spyOn(workspace, "rpc");
    vi.spyOn(workspace, "openProjectRecord").mockResolvedValue({
      id: "p",
      path: "/repo",
      worktrees: [{ id: "/repo", path: "/repo", branch: "main", isMain: true, locked: false, prunable: false }],
    name: "Repo",
  });
    await workspace.openProject("/repo");
    const id = workspace.active()!.id;
    rpc
      .mockResolvedValueOnce({ id: "accepted-session", options: { cwd: "/repo" } })
      .mockRejectedValueOnce(new RpcError("Provider unavailable", -32002));
    await expect(workspace.send(id, "Keep this draft")).rejects.toThrow(
      "Provider unavailable",
    );
    expect(workspace.active()).toMatchObject({
      sessionId: "accepted-session",
      state: "idle",
      title: "New chat",
    });
    rpc.mockResolvedValueOnce({ delivery: "accepted", recoveryRequired: false }).mockResolvedValueOnce({});
    await workspace.send(id, "Keep this draft");
    expect(rpc).toHaveBeenCalledWith("session/prompt", {
      sessionId: "accepted-session",
      text: "Keep this draft",
      idempotencyKey: expect.any(String),
    });
  });
  it("does not overwrite completion that arrives before the prompt acknowledgment", async () => {
    const workspace = new WorkspaceStore({ node: testNode });
    const rpc = vi.spyOn(workspace, "rpc");
    vi.spyOn(workspace, "openProjectRecord").mockResolvedValue({
      id: "p",
      path: "/repo",
      worktrees: [{ id: "/repo", path: "/repo", branch: "main", isMain: true, locked: false, prunable: false }],
    name: "Repo",
  });
    await workspace.openProject("/repo");
    const id = workspace.active()!.id;
    rpc.mockResolvedValueOnce({ id: "s", options: { cwd: "/repo" } }).mockImplementationOnce(async () => {
      workspace.updateThread(id, { state: "idle" });
      return { delivery: "accepted", recoveryRequired: false };
    });
    rpc.mockResolvedValueOnce({});
    await workspace.send(id, "A quick turn");
    expect(workspace.active()!.state).toBe("idle");
  });
});

it("an in-flight event page cannot reopen a closed session", async () => {
  vi.stubGlobal("localStorage", { getItem: () => null, setItem: vi.fn() });
  const workspace = new WorkspaceStore({ node: testNode });
  const rpc = vi.spyOn(workspace, "rpc");
  vi.spyOn(workspace, "openProjectRecord").mockResolvedValue({
    id: "p",
    path: "/repo",
    worktrees: [{ id: "/repo", path: "/repo", branch: "main", isMain: true, locked: false, prunable: false }],
  name: "Repo",
  });
  await workspace.openProject("/repo");
  const id = workspace.active()!.id;
  workspace.updateThread(id, { sessionId: "s" });
  rpc.mockImplementation(async (method) => {
    if (method === "runtime/initialize") return { name: "swarmie", protocolVersion: "1", runtimeIdentity: { identity: { environmentId: "e", storeId: "s", ownerScopeId: "o" }, incarnationId: "r" } } as any;
    if (method === "runtime/discover") return [{ harness: "codex", available: true }] as any;
    if (method === "extension/list") return [{ id: "git" }] as any;
    if (method === "project/list" || method === "session/list" || method === "session/transcript") return [] as any;
    if (method === "session/get" || method === "session/resume") return { id: "s", state: "idle", options: { cwd: "/repo" } } as any;
    if (method === "session/queue/list") return { items: [], paused: false } as any;
    return {} as any;
  });
  await workspace.connect();
  let resolvePage!: (events: Event[]) => void;
  rpc.mockImplementationOnce(
    () =>
      new Promise<Event[]>((resolve) => {
        resolvePage = resolve;
      }),
  );
  const polling = workspace.poll();
  rpc.mockResolvedValueOnce(null);
  await workspace.close(id);
  resolvePage([event(1, "state.changed", "idle")]);
  await polling;
  expect(workspace.active()!.state).toBe("closed");
});

describe("workspace continuity", () => {
  beforeEach(() =>
    vi.stubGlobal("localStorage", { getItem: () => null, setItem: vi.fn() }),
  );
  it("resumes a native saved session without clearing its transcript", async () => {
    const workspace = new WorkspaceStore({ node: testNode });
    workspace.createThread();
    const id = workspace.active()!.id;
    workspace.updateThread(id, {
      sessionId: "saved",
      worktreeId: "/repo",
      state: "closed",
      capabilities: { resume: { availability: "available" as const, source: "harness" as const, evidence: "declared" as const } },
      messages: [
        {
          id: "old",
          role: "user",
          content: [{ type: "text", text: "Prior context" }],
        },
      ],
    });
    const rpc = vi
      .spyOn(workspace, "rpc")
      .mockImplementation(async (method) =>
        method === "session/resume"
          ? { id: "saved", state: "idle", capabilities: { resume: { availability: "available" as const, source: "harness" as const, evidence: "declared" as const } } }
          : { delivery: "accepted", recoveryRequired: false },
      );
    await workspace.send(id, "Continue");
    expect(rpc).toHaveBeenCalledWith("session/resume", { sessionId: "saved" });
    expect(rpc).not.toHaveBeenCalledWith("session/create", expect.anything());
    expect(workspace.active()!.messages).toHaveLength(1);
  });
  it("does not substitute a new session when native resume is unavailable", async () => {
    const workspace = new WorkspaceStore({ node: testNode });
    workspace.createThread();
    const id = workspace.active()!.id;
    workspace.updateThread(id, {
      sessionId: "saved",
      worktreeId: "/repo",
      state: "closed",
      capabilities: { resume: { availability: "unsupported" as const, source: "harness" as const, evidence: "declared" as const } },
    });
    const rpc = vi.spyOn(workspace, "rpc");
    await expect(workspace.send(id, "Continue")).rejects.toThrow(
      "cannot resume",
    );
    expect(rpc).not.toHaveBeenCalled();
  });
  it("keeps a newer draft when a slower accepted send completes", async () => {
    const workspace = new WorkspaceStore({ node: testNode });
    workspace.createThread();
    const id = workspace.active()!.id;
    workspace.updateThread(id, {
      sessionId: "s",
      worktreeId: "/repo",
      title: "Existing",
      draft: "First",
    });
    vi.spyOn(workspace, "rpc").mockImplementation(async () => {
      workspace.updateThread(id, { draft: "Next message" });
      return { delivery: "accepted", recoveryRequired: false };
    });
    await workspace.send(id, "First");
    expect(workspace.active()!.draft).toBe("Next message");
  });
  it("queues explicitly without claiming to steer an unsupported harness", async () => {
    const workspace = new WorkspaceStore({ node: testNode });
    workspace.createThread();
    const id = workspace.active()!.id;
    workspace.updateThread(id, {
      sessionId: "s",
      worktreeId: "/repo",
      state: "running",
      title: "Existing",
      capabilities: { steering: { availability: "unsupported" as const, source: "harness" as const, evidence: "declared" as const } },
    });
    const rpc = vi
      .spyOn(workspace, "rpc")
      .mockImplementation(async (method) =>
        method === "session/queue/list" ? { items: [], paused: false } : {},
      );
    await expect(
      workspace.send(id, "Change course", [], "steer"),
    ).rejects.toThrow("does not support steering");
    await workspace.send(id, "Follow-up", [], "queue");
    expect(rpc).toHaveBeenCalledWith("session/queue", {
      sessionId: "s",
      text: "Follow-up",
    });
    expect(workspace.active()!.state).toBe("running");
  });
  it("restores durable pages then uses the current native state over historical close events", async () => {
    const workspace = new WorkspaceStore({ node: testNode });
    workspace.createThread();
    const id = workspace.active()!.id;
    workspace.updateThread(id, { sessionId: "s", worktreeId: "/repo" });
    const rpc = vi.spyOn(workspace, "rpc").mockImplementation(async (method) =>
      method === "session/get"
        ? { state: "idle", capabilities: { resume: { availability: "available" as const, source: "harness" as const, evidence: "declared" as const } } }
        : method === "session/transcript"
          ? [
              event(1, "message.user", {
                content: [{ type: "text", data: "History" }],
              }),
              event(2, "session.ended", {}),
            ]
          : { state: "idle", capabilities: { resume: { availability: "available" as const, source: "harness" as const, evidence: "declared" as const } } },
    );
    await workspace.restoreThread(id);
    expect(workspace.active()!.state).toBe("idle");
    expect(workspace.active()!.messages).toHaveLength(1);
    expect(rpc).toHaveBeenCalledWith("session/resume", { sessionId: "s" });
  });
});

it("renders rich event state without losing concurrent subagent updates", () => {
  const initial = { ...newThread("p", "w", "pi"), sessionId: "s" };
  const result = reduceEvents(initial, [
    event(1, "reasoning.delta", { text: "Thinking" }),
    event(2, "plan.updated", {
      steps: [{ title: "Inspect", status: "completed" }],
    }),
    event(3, "subagent.updated", {
      id: "child",
      name: "Reviewer",
      status: "running",
    }),
    event(4, "subagent.updated", { id: "child", status: "complete" }),
    event(5, "context.updated", {
      usedTokens: 2000,
      contextWindowTokens: 10000,
      percent: 20,
    }),
    event(6, "queue.changed", { items: [], paused: true }),
  ]);
  expect(result.reasoning).toBe("Thinking");
  expect(result.subagents).toHaveLength(1);
  expect(result.subagents![0].name).toBe("Reviewer");
  expect(result.context?.percent).toBe(20);
  expect(result.queuePaused).toBe(true);
});
it("replays a successful rewind without displaying provider-removed turns", () => {
  let thread: Thread = { ...newThread("p", "w", "codex"), sessionId: "s" };
  thread = reduceEvents(thread, [
    event(1, "message.user", { content: [{ type: "text", data: "First" }] }),
    event(2, "text.delta", { text: "First answer" }),
    event(3, "turn.ended", {}),
  ]);
  thread = reduceEvents(thread, [
    {
      ...event(4, "message.user", {
        content: [{ type: "text", data: "Second" }],
      }),
      turnId: "second",
    },
    { ...event(5, "text.delta", { text: "Second answer" }), turnId: "second" },
    event(6, "conversation.rolled_back", { turns: 1 }),
  ]);
  expect(thread.messages).toHaveLength(2);
  expect(thread.turnGroups).toHaveLength(1);
});
it("keeps image attachments visible after durable replay", () => {
  const thread = reduceEvents(
    { ...newThread("p", "w", "codex"), sessionId: "s" },
    [
      event(1, "message.user", {
        content: [
          { type: "text", data: "Describe it" },
          { type: "image", data: { mime_type: "image/png", data: "AAAA" } },
        ],
      }),
    ],
  );
  expect(thread.messages[0].content).toEqual([
    { type: "text", text: "Describe it" },
    { type: "image", image: "data:image/png;base64,AAAA" },
  ]);
});
it("an optional review failure cannot turn an accepted provider completion into a failed session", async () => {
  vi.stubGlobal("localStorage", { getItem: () => null, setItem: vi.fn() });
  const workspace = new WorkspaceStore({ node: testNode });
  workspace.createThread();
  const id = workspace.active()!.id;
  workspace.updateThread(id, { sessionId: "s", state: "running" });
  const rpc = vi
    .spyOn(workspace, "rpc")
    .mockImplementation(async (method) =>
      method === "runtime/initialize"
        ? { name: "swarmie", protocolVersion: "1", runtimeIdentity: { identity: { environmentId: "e", storeId: "s", ownerScopeId: "o" }, incarnationId: "r" } }
        : method === "runtime/discover"
        ? []
        : method === "extension/list"
          ? []
          : (method === "session/list" || method === "project/list")
            ? []
            : method === "session/get"
              ? { state: "running", capabilities: { resume: { availability: "available" as const, source: "harness" as const, evidence: "declared" as const } } }
              : method === "session/transcript"
                ? []
                : method === "session/resume"
                  ? { state: "running", capabilities: { resume: { availability: "available" as const, source: "harness" as const, evidence: "declared" as const } } }
                  : method === "session/queue/list"
                    ? { items: [], paused: false }
                    : method === "session/events"
                      ? [event(1, "turn.ended", {})]
                      : {},
    );
  await workspace.connect();
  vi.spyOn(workspace, "review").mockRejectedValue(
    new Error("Review temporarily unavailable"),
  );
  await workspace.poll();
  expect(workspace.active()!.state).toBe("idle");
  expect(workspace.getSnapshot().error).toBe("Review temporarily unavailable");
  expect(rpc).toHaveBeenCalledWith("session/events", expect.anything());
});

it("retains the persisted user event time without manufacturing a date for unknown history", () => {
  const thread = { ...newThread("", "/project", "codex"), sessionId: "native" };
  const timestampMs = Date.parse("2026-10-04T19:02:00Z");
  const result = reduceEvents(thread, [
    { sessionId: "native", sequence: 1, timestampMs, type: "message.user", data: { content: [{ type: "text", data: "Saved" }] } },
    { sessionId: "native", sequence: 2, type: "message.user", data: { content: [{ type: "text", data: "Unknown time" }] } },
  ]);
  expect(result.messages[0].createdAt?.getTime()).toBe(timestampMs);
  expect(result.messages[1].createdAt).toBeUndefined();
});
