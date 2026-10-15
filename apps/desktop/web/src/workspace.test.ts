import type { AgentTask } from "@swarmie/contracts";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { WorkspaceCache } from "./cache";
import { record } from "./checked-json";
import { hasConversation } from "./protocol";
import {
  deferred,
  fixtureOptions,
  fixtureCapabilities,
  fixtureSession,
  fixturePolicy,
  fixtureReceipt,
  fixtureProject,
  fixtureEvent,
  fixtureCapability,
} from "./test-fixtures";
import { WorkspaceStore, RpcError } from "./workspace";

const node = {
  environmentId: "view-node",
  storeId: "view-store",
  ownerScopeId: "view-owner",
};
const session = (id = "session") =>
  fixtureSession({
    id,
    state: "idle",
    options: fixtureOptions({
      harness: "mock",
      cwd: "/project",
      config: {},
    }),
    capabilities: fixtureCapabilities({}),
    sequence: 0,
  });
const projection = (snapshot = session(), extra = {}) => ({
  snapshot,
  entries: [],
  unresolved: false,
  events: [],
  activation: {
    status: "attached",
  },
  ...extra,
});
const policy = fixturePolicy({
  requested: null,
  controlledSettings: [],
});
beforeEach(() => {
  vi.stubGlobal("localStorage", {
    getItem: () => null,
    setItem: vi.fn(),
  });
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
function setup() {
  const store = new WorkspaceStore({
    node,
  });
  store.getSnapshot().connected = true;
  store.getSnapshot().chatDirectory = "/project";
  store.createThread();
  const { id } = requireFixtureValue(store.active());
  store.updateThread(id, {
    sessionId: "session",
    state: "idle",
    title: "Existing",
    harness: "mock",
  });
  const rpc = vi.spyOn(store, "request").mockImplementation(async (method) => {
    if (
      [
        "client/sessionOpen",
        "client/sessionProjection",
        "client/sessionRecover",
        "client/submissions",
      ].includes(method)
    ) {
      return projection();
    }
    if (method === "client/sessionQueue") {
      return {
        items: [],
        paused: false,
      };
    }
    if (method === "session/list") {
      return [];
    }
    throw new Error(`Unexpected ${method}`);
  });
  return {
    store,
    id,
    rpc,
  };
}
const accepted = (text: string, extra = {}) => ({
  input: {
    text,
    content: [
      {
        type: "text",
        data: text,
      },
    ],
    attachments: [],
    contextItems: [],
    afterSequence: 0,
  },
  receipt: fixtureReceipt({
    id: "receipt",
    sessionId: "session",
    delivery: "accepted",
    recoveryRequired: false,
    turnId: "turn",
  }),
  ...extra,
});
it("uses the canonical project returned by one application request", async () => {
  const { store, rpc } = setup();
  rpc.mockImplementation(async (method) =>
    method === "client/projectDefaults"
      ? {
          revision: 2,
          cwd: "/canonical",
        }
      : fixtureProject({
          id: "project",
          revision: 2,
          path: "/canonical",
          name: "Chosen name",
          worktrees: [],
        }),
  );
  expect(await store.openProject("/selected", "Chosen name")).toMatchObject({
    id: "project",
    name: "Chosen name",
  });
  expect(rpc).toHaveBeenCalledWith("client/projectOpen", {
    path: "/selected",
    name: "Chosen name",
  });
  await store.refreshProject("project");
  expect(rpc).toHaveBeenCalledWith("client/projectGet", {
    projectId: "project",
  });
  expect(
    rpc.mock.calls.some(
      ([method]) => method === "directory/register" || method === "project/create",
    ),
  ).toBe(false);
});
it("does not infer project membership from the path of an imported session", async () => {
  const { store, rpc } = setup();
  rpc.mockResolvedValue([session("imported")]);
  await store.restoreSessions();
  expect(store.getSnapshot().threads.find((t) => t.sessionId === "imported")).toMatchObject({
    projectId: "",
    worktreeId: "/project",
  });
});
it("waits for defaults while typing and deduplicates simultaneous session launches", async () => {
  const { store, rpc } = setup();
  store.getSnapshot().projects = [
    fixtureProject({
      id: "project",
      name: "Project",
      revision: 1,
      path: "/project",
      worktrees: [
        {
          id: "/project",
          path: "/project",
          branch: null,
          isMain: true,
          locked: false,
          prunable: false,
        },
      ],
    }),
  ];
  const resolveGate = deferred<unknown>();
  const { resolve } = resolveGate;
  rpc.mockImplementation(async (method) => {
    if (method === "client/projectDefaults") {
      return await resolveGate.request();
    }
    if (method === "client/sessionCreate") {
      return session("created");
    }
    throw new Error(`Unexpected ${method}`);
  });
  store.createThread("project");
  const { id } = requireFixtureValue(store.active());
  const first = store.ensureSession(id);
  const second = store.ensureSession(id);
  store.updateThread(id, {
    draft: "Typed during defaults",
  });
  resolve({
    revision: 1,
    cwd: "/project",
    harness: "mock",
    model: "selected",
  });
  await Promise.all([first, second]);
  expect(rpc.mock.calls.filter(([method]) => method === "client/sessionCreate")).toHaveLength(1);
  expect(store.active()).toMatchObject({
    sessionId: "created",
    draft: "Typed during defaults",
    model: "selected",
  });
});
it("hydrates a closed conversation and dates without view-owned attachment decisions", async () => {
  const { store, id, rpc } = setup();
  rpc.mockResolvedValue(
    projection(
      {
        ...session(),
        state: "closed",
        sequence: 1,
        metadata: {
          title: null,
          archived: false,
          forkOrigin: null,
          createdAtMs: 10,
          updatedAtMs: 20,
        },
      },
      {
        activation: {
          status: "inactive",
        },
        events: [
          fixtureEvent(
            {
              type: "message.user",
              data: {
                content: [
                  {
                    type: "text",
                    data: "Saved",
                  },
                ],
              },
            },
            {
              sessionId: "session",
              sequence: 1,
              timestampMs: 1000,
            },
          ),
        ],
      },
    ),
  );
  await store.restoreThread(id);
  expect(rpc).toHaveBeenCalledExactlyOnceWith("client/sessionOpen", {
    sessionId: "session",
  });
  expect(store.active()).toMatchObject({
    state: "closed",
    createdAtMs: 10,
    updatedAtMs: 20,
  });
  expect(
    new Date(
      requireFixtureValue(requireFixtureValue(store.active()).messages[0].createdAt),
    ).getTime(),
  ).toBe(1000);
});
it("backfills message dates from one verified projection without resuming", async () => {
  const { store, id, rpc } = setup();
  store.updateThread(id, {
    messages: [
      {
        id: "user-1",
        role: "user",
        content: "Saved",
      },
    ],
  });
  rpc.mockResolvedValue(
    projection(session(), {
      events: [
        fixtureEvent(
          {
            type: "message.user",
            data: {
              content: [],
            },
          },
          {
            sessionId: "session",
            sequence: 1,
            timestampMs: 1000,
          },
        ),
      ],
    }),
  );
  await store.loadMessageTimes(id);
  expect(rpc).toHaveBeenCalledExactlyOnceWith("client/sessionProjection", {
    sessionId: "session",
  });
  expect(
    new Date(
      requireFixtureValue(requireFixtureValue(store.active()).messages[0].createdAt),
    ).getTime(),
  ).toBe(1000);
});
it("restores native task state from the session projection and removes stale cached tasks", async () => {
  const { store, id, rpc } = setup();
  store.updateThread(id, {
    tasks: [
      {
        id: "stale-task",
        description: "Cached task",
        status: "running",
      },
    ],
  });
  const task: AgentTask = {
    kind: null,
    summary: null,
    id: "observed-task",
    description: "Inspect changes",
    status: "completed",
  };
  rpc.mockResolvedValue(
    projection(session(), {
      events: [
        fixtureEvent(
          {
            type: "task.updated",
            data: task,
          },
          {
            sessionId: "session",
            sequence: 1,
          },
        ),
        fixtureEvent(
          {
            type: "task.updated",
            data: {
              ...task,
              id: "foreign-task",
            },
          },
          {
            sessionId: "other-session",
            sequence: 2,
          },
        ),
        fixtureEvent(
          {
            type: "task.updated",
            data: {
              ...task,
              status: "running",
            },
          },
          {
            sessionId: "session",
            sequence: 1,
          },
        ),
      ],
    }),
  );
  await store.restoreThread(id);
  expect(requireFixtureValue(store.active()).tasks).toEqual([task]);
  expect(rpc.mock.calls.some(([method]) => method === "session/task/stop")).toBe(false);
});
it("discards timestamp hydration after the displayed session changes", async () => {
  const { store, id, rpc } = setup();
  const finishGate = deferred<unknown>();
  const finish = finishGate.resolve;
  rpc.mockImplementationOnce(async () => await finishGate.request());
  const pending = store.loadMessageTimes(id);
  store.updateThread(id, {
    sessionId: "other",
    messages: [
      {
        id: "user-1",
        role: "user",
        content: "Other",
      },
    ],
  });
  finish(
    projection(session(), {
      events: [
        fixtureEvent(
          {
            type: "message.user",
            data: {
              content: [],
            },
          },
          {
            sessionId: "session",
            sequence: 1,
            timestampMs: 1000,
          },
        ),
      ],
    }),
  );
  await pending;
  expect(requireFixtureValue(store.active()).messages[0].createdAt).toBeUndefined();
});
it("keeps composer and file edits made during an asynchronous projection", async () => {
  const { store, id, rpc } = setup();
  const finishGate = deferred<unknown>();
  const finish = finishGate.resolve;
  rpc.mockImplementationOnce(async () => await finishGate.request());
  const pending = store.restoreThread(id);
  store.updateThread(id, {
    draft: "New draft",
    fileDrafts: {
      "notes.txt": {
        original: "old",
        content: "edited",
      },
    },
  });
  finish(projection());
  await pending;
  expect(store.active()).toMatchObject({
    draft: "New draft",
    fileDrafts: {
      "notes.txt": {
        content: "edited",
      },
    },
  });
});
it("sends authored intent and lets the application interpret skills and attachments", async () => {
  const { store, id, rpc } = setup();
  const attachment = {
    id: "file",
    name: "document.pdf",
    mime: "application/pdf",
    image: false,
    binary: true,
    data: "ZmlsZQ==",
  };
  const context = {
    id: "skill",
    kind: "resource" as const,
    label: "Skill",
    text: "Selected",
    skill: {
      name: "selected",
      path: "/skills/selected",
    },
  };
  store.updateThread(id, {
    draft: "Original",
    attachments: [attachment],
    contextItems: [context],
  });
  rpc.mockImplementationOnce(async () => accepted("Original"));
  await store.send(id, "Original", [attachment]);
  expect(rpc).toHaveBeenCalledTimes(1);
  const [method, params] = requireFixtureValue(rpc.mock.calls[0]);
  expect(method).toBe("client/sessionInput");
  expect(params).toMatchObject({
    sessionId: "session",
    text: "Original",
    attachments: [attachment],
    contextItems: [context],
    delivery: "prompt",
  });
  expect(record(params).idempotencyKey).toBeTypeOf("string");

  expect(store.active()).toMatchObject({
    draft: "",
    attachments: [],
    contextItems: [],
    submitted: true,
  });
});
it("keeps a newer draft and early completion when input acknowledgment is delayed", async () => {
  const { store, id, rpc } = setup();
  store.updateThread(id, {
    draft: "Original",
  });
  rpc.mockImplementationOnce(async () => {
    store.updateThread(id, {
      draft: "New draft",
      state: "idle",
    });
    return accepted("Original");
  });
  await store.send(id, "Original");
  expect(store.active()).toMatchObject({
    draft: "New draft",
    state: "idle",
    submitted: true,
  });
});
it("retains a session and its draft when the application rejects input without effects", async () => {
  const { store, id, rpc } = setup();
  store.updateThread(id, {
    draft: "Original",
  });
  rpc.mockRejectedValueOnce(
    new RpcError("Provider unavailable", -32_001, {
      effectKnowledge: "none",
    }),
  );
  await expect(store.send(id, "Original")).rejects.toThrow("Provider unavailable");
  expect(store.active()).toMatchObject({
    sessionId: "session",
    state: "idle",
    draft: "Original",
    submissionKeys: [],
  });
  rpc.mockResolvedValueOnce(accepted("Original"));
  await store.send(id, "Original");
  expect(hasConversation(requireFixtureValue(store.active()))).toBe(true);
});
it("recovers lost confirmation with reads and keeps a newer draft", async () => {
  const { store, id, rpc } = setup();
  rpc.mockImplementation(async (method) => {
    if (method === "client/sessionInput") {
      store.updateThread(id, {
        draft: "New draft",
      });
      throw new Error("Response lost");
    }
    if (method === "client/submissions") {
      const [{ key }] = requireFixtureValue(requireFixtureValue(store.active()).submissionKeys);
      return projection(session(), {
        entries: [
          {
            key,
            sessionId: "session",
            input: {
              ...accepted("Original").input,
            },
            receipt: {
              ...accepted("Original").receipt,
              idempotencyKey: key,
            },
          },
        ],
      });
    }
    throw new Error(`Unexpected ${method}`);
  });
  await expect(store.send(id, "Original")).rejects.toThrow("Response lost");
  expect(store.active()).toMatchObject({
    state: "idle",
    draft: "New draft",
    submitted: true,
  });
  expect(rpc.mock.calls.filter(([method]) => method === "client/sessionInput")).toHaveLength(1);
});
it("checks unresolved delivery through one application operation", async () => {
  const { store, id, rpc } = setup();
  rpc.mockResolvedValueOnce(
    projection(session(), {
      unresolved: true,
      activation: {
        status: "blocked",
      },
      entries: [
        {
          key: "original",
          sessionId: "session",
          receipt: fixtureReceipt({
            delivery: "unknown",
            recoveryRequired: true,
          }),
        },
      ],
    }),
  );
  await store.checkSubmissionRecovery(id);
  expect(rpc).toHaveBeenCalledExactlyOnceWith("client/sessionRecover", {
    sessionId: "session",
  });
  expect(requireFixtureValue(store.active()).state).toBe("needs_input");
});
it("updates queue presentation with one operation and no cached prompt reconstruction", async () => {
  const { store, id, rpc } = setup();
  rpc.mockResolvedValueOnce({
    items: [
      {
        id: "queued",
        createdAtMs: 0,
        prompt: {
          content: [
            {
              type: "text",
              data: "Updated",
            },
          ],
        },
      },
    ],
    paused: true,
  });
  await store.editQueue(id, "queued", "Updated");
  expect(rpc).toHaveBeenCalledExactlyOnceWith("client/sessionQueue", {
    sessionId: "session",
    id: "queued",
    action: "update",
    text: "Updated",
  });
  expect(store.active()).toMatchObject({
    queuePaused: true,
    queue: [
      {
        id: "queued",
      },
    ],
  });
});
it("renders an edit draft and projection without running rollback or asset reads in the view", async () => {
  const { store, id, rpc } = setup();
  rpc.mockResolvedValueOnce({
    projection: projection(),
    draft: {
      draft: "Original",
      attachments: [
        {
          id: "image",
          name: "image.png",
          mime: "image/png",
          image: true,
          data: "aW1hZ2U=",
        },
      ],
      contextItems: [],
    },
  });
  await store.editFrom(id, "user-2", "conversation_and_files");
  expect(rpc).toHaveBeenCalledExactlyOnceWith("client/sessionHistory", {
    sessionId: "session",
    action: "edit",
    messageId: "user-2",
    scope: "conversation_and_files",
  });
  expect(store.active()).toMatchObject({
    draft: "Original",
    attachments: [
      {
        id: "image",
      },
    ],
  });
});
it("sends only a native tree anchor when creating an edit branch", async () => {
  const { store, id, rpc } = setup();
  rpc.mockResolvedValueOnce({
    projection: projection(session("child")),
    draft: {
      draft: "Native original",
      attachments: [],
      contextItems: [],
    },
  });
  const child = await store.editInBranch(id, {
    id: "native-anchor",
    text: "Untrusted view text",
  });
  expect(rpc).toHaveBeenCalledExactlyOnceWith("client/sessionHistory", {
    sessionId: "session",
    action: "branch",
    messageId: "native-anchor",
  });
  expect(store.active()).toMatchObject({
    id: child,
    sessionId: "child",
    draft: "Native original",
  });
});
it("keeps draft and navigation unchanged after a failed native history operation", async () => {
  const { store, id, rpc } = setup();
  store.updateThread(id, {
    draft: "Keep draft",
  });
  rpc.mockRejectedValueOnce(new RpcError("Saved attachment unavailable", -32_001));
  await expect(store.editFrom(id, "user-2")).rejects.toThrow("Saved attachment unavailable");
  expect(rpc).toHaveBeenCalledTimes(1);
  expect(store.active()).toMatchObject({
    id,
    draft: "Keep draft",
  });
});
it("does not apply an old event page after closing its session", async () => {
  const { store, id, rpc } = setup();
  const finishGate = deferred<unknown>();
  const finish = finishGate.resolve;
  rpc.mockImplementationOnce(async () => await finishGate.request());
  const pending = store.poll();
  rpc.mockResolvedValueOnce(null);
  await store.close(id);
  finish([
    fixtureEvent(
      {
        type: "state.changed",
        data: "idle",
      },
      {
        sessionId: "session",
        sequence: 1,
      },
    ),
  ]);
  await pending;
  expect(requireFixtureValue(store.active()).state).toBe("closed");
});
it("does not turn an accepted completion into failure when optional review fails", async () => {
  const { store, id, rpc } = setup();
  store.updateThread(id, {
    state: "running",
  });
  rpc.mockResolvedValueOnce([
    fixtureEvent(
      {
        type: "turn.ended",
        data: {
          outcome: "completed",
        },
      },
      {
        sessionId: "session",
        sequence: 1,
      },
    ),
  ]);
  vi.spyOn(store, "review").mockRejectedValue(new Error("Review unavailable"));
  await store.poll();
  expect(requireFixtureValue(store.active()).state).toBe("idle");
  expect(store.getSnapshot().error).toContain("Review unavailable");
});
it("discards a control preview returned after the draft changes harness", async () => {
  const { store, id, rpc } = setup();
  store.updateThread(id, {
    sessionId: undefined,
    harness: "mock",
  });
  const finishGate = deferred<unknown>();
  const finish = finishGate.resolve;
  rpc.mockImplementationOnce(async () => await finishGate.request());
  const pending = store.previewControls(id);
  await vi.waitFor(() => {
    expect(finishGate.started()).toBe(true);
  });
  store.updateThread(id, {
    harness: "other",
  });
  finish({
    executionPolicy: fixturePolicy(),
    configuration: null,
    models: null,
    capabilities: fixtureCapabilities({
      images: fixtureCapability("available"),
    }),
  });
  await pending;
  expect(requireFixtureValue(store.active()).capabilities).toBeUndefined();
});
it("applies observed configuration without another view-owned recovery call", async () => {
  const { store, id, rpc } = setup();
  rpc.mockRejectedValueOnce(
    new RpcError("Partial update", -32_001, {
      controls: {
        configuration: [],
        models: [],
        values: {
          model: "observed",
        },
        executionPolicy: policy,
      },
    }),
  );
  await expect(
    store.configureMany(id, {
      model: "selected",
    }),
  ).rejects.toThrow("Partial update");
  expect(store.active()).toMatchObject({
    model: "observed",
    config: {
      model: "observed",
    },
  });
  expect(rpc).toHaveBeenCalledTimes(1);
});
it("keeps invalid directory selections out of presentation settings", async () => {
  const { store, rpc } = setup();
  rpc.mockRejectedValueOnce(new RpcError("Folder missing", -32_001));
  await expect(store.setChatDirectory("/missing")).rejects.toThrow("Folder missing");
  expect(store.getSnapshot().chatDirectory).toBe("/project");
});
function requireFixtureValue<T>(value: T | null | undefined): T {
  if (value === undefined || value === null) {
    throw new Error("Expected the fixture value to be present.");
  }
  return value;
}

it("keeps the current workspace when native project data violates the directory contract", async () => {
  const { store, id, rpc } = setup();
  rpc.mockResolvedValue({
    id: "invalid-project",
    revision: 1,
    name: "Invalid",
    path: 123,
    git: false,
    worktrees: [],
  });
  expect(await store.openProject("/requested-directory")).toBeUndefined();
  expect(store.getSnapshot().projects).toEqual([]);
  expect(store.active()?.id).toBe(id);
  expect(store.getSnapshot().threads).toHaveLength(1);
  expect(store.getSnapshot().error).toContain(
    "Invalid native client response for client/projectOpen",
  );
  expect(rpc).toHaveBeenCalledExactlyOnceWith("client/projectOpen", {
    path: "/requested-directory",
  });
});
