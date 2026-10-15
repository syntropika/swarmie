import "fake-indexeddb/auto";
import type { LocalStoreIdentity, RuntimeIdentity } from "@swarmie/contracts";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { record } from "~/web/src/checked-json";
import { nodeKey, routeTargetKey, workspaceStorageKey } from "~/web/src/node-scope";
import type { fixtureRequest } from "~/web/src/test-fixtures";
import {
  deferred,
  fixtureRequestBody,
  fixtureProject,
  fixtureEvent,
  fixtureReceipt,
  fixtureInput,
} from "~/web/src/test-fixtures";
import { WorkspaceStore } from "~/web/src/workspace";

const a: LocalStoreIdentity = {
  environmentId: "node-a",
  storeId: "store-a",
  ownerScopeId: "owner-a",
};
const b: LocalStoreIdentity = {
  environmentId: "node-b",
  storeId: "store-b",
  ownerScopeId: "owner-b",
};
const targets = {
  "/nodes/a/rpc": {
    identity: a,
    incarnationId: "a-first",
  },
  "/nodes/b/rpc": {
    identity: b,
    incarnationId: "b-first",
  },
} satisfies Record<string, RuntimeIdentity>;
let local: Map<string, string>;
let requests: {
  path: string;
  method: string;
  expectedRuntime?: RuntimeIdentity;
}[];
let waitForEvents: (() => Promise<unknown>) | undefined;
beforeEach(() => {
  local = new Map();
  requests = [];
  waitForEvents = undefined;
  targets["/nodes/a/rpc"] = {
    identity: a,
    incarnationId: "a-first",
  };
  vi.stubGlobal("localStorage", {
    getItem: (key: string) => local.get(key) ?? null,
    setItem: (key: string, value: string) => local.set(key, value),
  });
  vi.stubGlobal("window", {
    addEventListener: vi.fn(),
    dispatchEvent: vi.fn(),
  });
  vi.stubGlobal(
    "fetch",
    vi.fn(async (path: string, options: RequestInit) => {
      const request = fixtureRequestBody(options);
      requests.push({
        path,
        ...request,
      });
      const result = await (async () => {
        if (request.method === "runtime/initialize") {
          return {
            name: "swarmie",
            protocolVersion: "1",
            runtimeIdentity: Object.entries(targets).find(([route]) => route === path)?.[1],
          };
        }
        if (request.method === "client/chatDirectory") {
          return {
            id: "directory",
            revision: 1,
            path: "/work",
            environmentId: "node-a",
            fingerprint: null,
            ownership: "user",
          };
        }
        if (request.method === "session/events" && waitForEvents) {
          return await waitForEvents();
        }
        if (request.method === "session/queue/list") {
          return {
            items: [],
            paused: false,
          };
        }
        return [];
      })();
      return Response.json({
        jsonrpc: "2.0",
        id: request.id,
        result,
      });
    }),
  );
});
afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});
it("keeps equal project, path, thread, session, and cursor IDs separate in both persistent stores", async () => {
  const first = new WorkspaceStore({
    node: a,
    rpcPath: "/nodes/a/rpc",
  });
  const second = new WorkspaceStore({
    node: b,
    rpcPath: "/nodes/b/rpc",
  });
  await Promise.all([first.ready(), second.ready()]);
  for (const [store, label, cursor] of [
    [first, "A", 10],
    [second, "B", 90],
  ] as const) {
    store.getSnapshot().projects = [
      fixtureProject({
        id: "project",
        name: `Project ${label}`,
        path: "/work",
        worktrees: [],
      }),
    ];
    store.createThread("project");
    const { id } = requireFixtureValue(store.active());
    store.updateThread(id, {
      id: "thread",
      sessionId: "session",
      sequence: cursor,
      draft: `Draft ${label}`,
      messages: [
        {
          id: "user-1",
          role: "user",
          content: [
            {
              type: "text",
              text: `Message ${label}`,
            },
          ],
        },
      ],
      attachments: [
        {
          id: "attachment",
          name: "file.txt",
          mime: "text/plain",
          data: label,
          image: false,
        },
      ],
      fileDrafts: {
        "file.txt": {
          content: label,
          original: "original",
        },
      },
      submissionKeys: [
        {
          key: "submission",
          recordId: `record-${label}`,
        },
      ],
    });
    store.select("thread");
  }
  await Promise.all([first.flushPersistence(), second.flushPersistence()]);
  const restoredA = new WorkspaceStore({
    node: a,
  });
  const restoredB = new WorkspaceStore({
    node: b,
  });
  await Promise.all([restoredA.ready(), restoredB.ready()]);
  expect(restoredA.getSnapshot().projects[0].name).toBe("Project A");
  expect(restoredB.getSnapshot().projects[0].name).toBe("Project B");
  expect(restoredA.getSnapshot().error).toBe("");
  expect(restoredB.getSnapshot().error).toBe("");
  expect(restoredA.active()).toMatchObject({
    draft: "Draft A",
    sequence: 10,
    attachments: [
      {
        data: "A",
      },
    ],
    submissionKeys: [
      {
        recordId: "record-A",
      },
    ],
  });
  expect(restoredB.active()).toMatchObject({
    draft: "Draft B",
    sequence: 90,
    attachments: [
      {
        data: "B",
      },
    ],
    submissionKeys: [
      {
        recordId: "record-B",
      },
    ],
  });
  expect(requireFixtureValue(restoredA.active()).fileDrafts?.["file.txt"].content).toBe("A");
  expect(requireFixtureValue(restoredB.active()).messages[0].content).toEqual([
    {
      type: "text",
      text: "Message B",
    },
  ]);
  const savedA: unknown = JSON.parse(requireFixtureValue(local.get(workspaceStorageKey(a))));
  expect(record(savedA).node).toEqual(a);
  const savedB: unknown = JSON.parse(requireFixtureValue(local.get(workspaceStorageKey(b))));
  expect(record(savedB).node).toEqual(b);
});
it("applies a delayed response only to the originating node while another node is viewed", async () => {
  const first = new WorkspaceStore({
    node: a,
    rpcPath: "/nodes/a/rpc",
  });
  const second = new WorkspaceStore({
    node: b,
    rpcPath: "/nodes/b/rpc",
  });
  await Promise.all([first.connect(), second.connect()]);
  for (const store of [first, second]) {
    store.updateThread(requireFixtureValue(store.active()).id, {
      sessionId: "session",
      state: "running",
      sequence: 0,
    });
  }
  const finishGate = deferred<unknown>();
  const finish = finishGate.resolve;
  waitForEvents = async () => await finishGate.request();
  let viewed = first;
  const pending = first.poll();
  await vi.waitFor(() => {
    expect(finishGate.started()).toBe(true);
  });
  viewed = second;
  finish([
    fixtureEvent(
      {
        type: "text.delta",
        data: {
          item_id: null,
          text: "Only A",
        },
      },
      {
        sessionId: "session",
        sequence: 1,
      },
    ),
  ]);
  await pending;
  expect(requireFixtureValue(first.active()).messages[0].content).toEqual([
    {
      type: "text",
      text: "Only A",
    },
  ]);
  expect(requireFixtureValue(viewed.active()).messages).toEqual([]);
  expect(requireFixtureValue(viewed.active()).sequence).toBe(0);
  expect(requests.find((request) => request.method === "session/events")).toMatchObject({
    path: "/nodes/a/rpc",
    expectedRuntime: targets["/nodes/a/rpc"],
  });
});
it("pins offline drafts to their known node and rejects another node at that route", async () => {
  const store = new WorkspaceStore({
    node: a,
    rpcPath: "/nodes/a/rpc",
  });
  await store.connect();
  store.updateThread(requireFixtureValue(store.active()).id, {
    draft: "Keep on A",
  });
  await store.flushPersistence();
  const restored = new WorkspaceStore({
    rpcPath: "/nodes/a/rpc",
  });
  await restored.ready();
  expect(restored.getSnapshot().node).toEqual(a);
  expect(requireFixtureValue(restored.active()).draft).toBe("Keep on A");
  targets["/nodes/a/rpc"] = targets["/nodes/b/rpc"];
  requests = [];
  await restored.connect();
  expect(restored.getSnapshot().connected).toBe(false);
  expect(restored.getSnapshot().error).toContain("different environment");
  expect(requests.map((request) => request.method)).toEqual(["runtime/initialize"]);
  expect(requireFixtureValue(restored.active()).draft).toBe("Keep on A");
  expect(JSON.parse(requireFixtureValue(local.get(routeTargetKey("/nodes/a/rpc"))))).toEqual(a);
});
it("discards responses and failures from an earlier connection without changing the reconnected node", async () => {
  const store = new WorkspaceStore({
    node: a,
    rpcPath: "/nodes/a/rpc",
  });
  await store.connect();
  const finishGate = deferred<unknown>();
  const finish = finishGate.resolve;
  waitForEvents = async () => await finishGate.request();
  const pending = store.rpc("session/events", {
    sessionId: "session",
  });
  const rejected = expect(pending).rejects.toThrow("earlier runtime connection");
  targets["/nodes/a/rpc"] = {
    identity: a,
    incarnationId: "a-restarted",
  };
  await store.connect();
  finish([
    fixtureEvent(
      {
        type: "text.delta",
        data: {
          item_id: null,
          text: "Stale",
        },
      },
      {
        sessionId: "session",
        sequence: 1,
      },
    ),
  ]);
  await rejected;
  expect(store.getSnapshot().connected).toBe(true);
  expect(store.getSnapshot().error).toBe("");
  expect(requireFixtureValue(store.active()).messages).toEqual([]);
  const failGate = deferred<Response>();
  const fail = failGate.reject;
  vi.mocked(fetch).mockImplementationOnce(async () => await failGate.request());
  const failed = store.rpc("project/list");
  const rejectedFailure = expect(failed).rejects.toThrow("earlier runtime connection");
  await store.connect();
  fail(new TypeError("late transport failure"));
  await rejectedFailure;
  expect(store.getSnapshot().connected).toBe(true);
});
it("uses the full stable tuple and rejects cached metadata with foreign identity", async () => {
  const other = {
    ...a,
    storeId: "other-store",
  };
  expect(nodeKey(a)).not.toBe(nodeKey(other));
  expect(
    nodeKey({
      ...a,
      environmentId: "a:b",
      storeId: "c",
    }),
  ).not.toBe(
    nodeKey({
      ...a,
      environmentId: "a",
      storeId: "b:c",
    }),
  );
  local.set(
    workspaceStorageKey(a),
    JSON.stringify({
      version: 3,
      node: b,
      projects: [
        {
          id: "foreign",
        },
      ],
      threads: [],
    }),
  );
  const store = new WorkspaceStore({
    node: a,
  });
  await store.ready();
  expect(store.getSnapshot().projects).toEqual([]);
  expect(store.getSnapshot().node).toEqual(a);
  expect(
    () =>
      new WorkspaceStore({
        rpcPath: "https://remote.example/rpc",
      }),
  ).toThrow("same-origin");
});
it("keeps legacy unscoped records untouched and binds the first verified route without loading their history", async () => {
  const old = JSON.stringify({
    version: 2,
    projects: [
      {
        id: "old",
      },
    ],
    threads: [
      {
        id: "old-thread",
        sessionId: "session",
      },
    ],
  });
  local.set("swarmie.workspace.v2", old);
  local.set("swarmie.runtime.target", JSON.stringify(b));
  const store = new WorkspaceStore({
    rpcPath: "/nodes/a/rpc",
  });
  await store.ready();
  expect(store.getSnapshot().node).toBeUndefined();
  expect(store.getSnapshot().threads).toEqual([]);
  await store.connect();
  expect(store.getSnapshot().node).toEqual(a);
  expect(store.getSnapshot().threads.some((thread) => thread.id === "old-thread")).toBe(false);
  await store.flushPersistence();
  expect(local.get("swarmie.workspace.v2")).toBe(old);
  expect(local.get("swarmie.runtime.target")).toBe(JSON.stringify(b));
  expect(local.has(workspaceStorageKey(a))).toBe(true);
});
it("keeps a pending prompt and its acceptance receipt on the original node when the viewed node changes", async () => {
  const first = new WorkspaceStore({
    node: a,
    rpcPath: "/nodes/a/rpc",
  });
  const second = new WorkspaceStore({
    node: b,
    rpcPath: "/nodes/b/rpc",
  });
  first.getSnapshot().chatDirectory = "/work";
  second.getSnapshot().chatDirectory = "/work";
  await Promise.all([first.connect(), second.connect()]);
  for (const store of [first, second]) {
    store.updateThread(requireFixtureValue(store.active()).id, {
      sessionId: "session",
      state: "idle",
      draft: "Next instruction",
    });
  }
  const previous = requireFixtureValue(vi.mocked(fetch).getMockImplementation());
  const finishGate = deferred<Response>();
  const finish = finishGate.resolve;
  let prompt: ReturnType<typeof fixtureRequest> | undefined;
  vi.mocked(fetch).mockImplementation(async (path, options) => {
    const request = fixtureRequestBody(options);
    if (request.method !== "client/sessionInput") {
      return await previous(path, options);
    }
    expect(path).toBe("/nodes/a/rpc");
    prompt = request;
    return await finishGate.request();
  });
  let viewed = first;
  const pending = first.send(requireFixtureValue(first.active()).id, "Next instruction");
  await vi.waitFor(() => {
    expect(prompt).toBeDefined();
  });
  viewed = second;
  finish(
    Response.json({
      result: {
        input: fixtureInput("Next instruction"),
        receipt: fixtureReceipt({
          delivery: "accepted",
          recoveryRequired: false,
        }),
      },
    }),
  );
  await pending;
  expect(requireFixtureValue(prompt).expectedRuntime).toEqual(targets["/nodes/a/rpc"]);
  expect(first.active()).toMatchObject({
    draft: "",
    submitted: true,
  });
  expect(requireFixtureValue(first.active()).submissionKeys?.[0]).toMatchObject({
    key: requireFixtureValue(prompt).params.idempotencyKey,
    receipt: {
      delivery: "accepted",
    },
  });
  expect(viewed.active()).toMatchObject({
    draft: "Next instruction",
  });
  expect(requireFixtureValue(viewed.active()).submissionKeys).toBeUndefined();
  await Promise.all([first.flushPersistence(), second.flushPersistence()]);
});
function requireFixtureValue<T>(value: T | null | undefined): T {
  if (value === undefined || value === null) {
    throw new Error("Expected the fixture value to be present.");
  }
  return value;
}
