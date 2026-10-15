import type { RuntimeIdentity } from "@swarmie/contracts";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { WorkspaceCache } from "./cache";
import { routeTargetKey } from "./node-scope";
import {
  fixtureRequestBody,
  fixtureOptions,
  fixtureCapabilities,
  fixtureSession,
} from "./test-fixtures";
import { WorkspaceStore } from "./workspace";

const identity: RuntimeIdentity = {
  identity: {
    environmentId: "environment",
    storeId: "store",
    ownerScopeId: "owner",
  },
  incarnationId: "first-owner",
};
let target: RuntimeIdentity;
let offline: boolean;
let requests: {
  method: string;
  expectedRuntime?: RuntimeIdentity;
}[];
let saved: Map<string, string>;
beforeEach(() => {
  target = structuredClone(identity);
  offline = false;
  requests = [];
  saved = new Map();
  vi.stubGlobal("localStorage", {
    getItem: (key: string) => saved.get(key) ?? null,
    setItem: (key: string, value: string) => saved.set(key, value),
  });
  vi.stubGlobal("window", {
    addEventListener: vi.fn(),
    dispatchEvent: vi.fn(),
  });
  vi.spyOn(WorkspaceCache.prototype, "load").mockResolvedValue(undefined);
  vi.spyOn(WorkspaceCache.prototype, "save").mockResolvedValue();
  vi.stubGlobal(
    "fetch",
    vi.fn(async (_url: string, options: RequestInit) => {
      if (offline) {
        throw new TypeError("connection lost");
      }
      const request = fixtureRequestBody(options);
      requests.push(request);
      const result = (() => {
        if (request.method === "runtime/initialize") {
          return {
            name: "swarmie",
            protocolVersion: "1",
            runtimeIdentity: target,
          };
        }
        if (request.method === "client/sessionOpen") {
          return {
            snapshot: fixtureSession({
              id: "session",
              state: "running",
              sequence: 0,
              options: fixtureOptions({
                harness: "mock",
                cwd: "/work",
              }),
              capabilities: fixtureCapabilities({}),
            }),
            events: [],
            entries: [],
            unresolved: false,
            activation: {
              status: "attached",
            },
          };
        }
        if (request.method === "session/queue/list") {
          return {
            items: [],
            paused: false,
          };
        }
        if (request.method === "session/get" || request.method === "session/resume") {
          return fixtureSession({
            id: "session",
            state: "running",
            options: fixtureOptions({
              harness: "mock",
              cwd: "/work",
            }),
            capabilities: fixtureCapabilities({}),
          });
        }
        return [];
      })();
      return Response.json(
        {
          jsonrpc: "2.0",
          id: request.id,
          result,
        },
        {
          headers: {
            "Content-Type": "application/json",
          },
        },
      );
    }),
  );
});
afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});
it("pins commands to the verified owner and accepts a restarted owner of the same Environment", async () => {
  const store = new WorkspaceStore();
  store.getSnapshot().chatDirectory = "/work";
  await Promise.all([store.connect(), store.connect()]);
  expect(requests.filter((r) => r.method === "runtime/initialize")).toHaveLength(1);
  expect(requests.find((r) => r.method === "client/projectCatalog")?.expectedRuntime).toEqual(
    identity,
  );
  store.updateThread(requireFixtureValue(store.active()).id, {
    draft: "Keep this draft",
  });
  target = {
    ...identity,
    incarnationId: "restarted-owner",
  };
  await store.connect();
  await store.rpc("project/list");
  expect(requests.at(-1)?.expectedRuntime?.incarnationId).toBe("restarted-owner");
  expect(requireFixtureValue(store.active()).draft).toBe("Keep this draft");
  expect(saved.get(routeTargetKey("/rpc"))).not.toContain("incarnationId");
});
it("blocks a different retained Environment before discovering or mutating it", async () => {
  const store = new WorkspaceStore();
  store.getSnapshot().chatDirectory = "/work";
  await store.connect();
  store.updateThread(requireFixtureValue(store.active()).id, {
    draft: "My unsent work",
  });
  target = {
    ...identity,
    identity: {
      ...identity.identity,
      storeId: "other-store",
    },
  };
  requests = [];
  await store.connect();
  expect(store.getSnapshot().connected).toBe(false);
  expect(store.getSnapshot().error).toContain("different environment");
  expect(requests.map((r) => r.method)).toEqual(["runtime/initialize"]);
  await expect(
    store.rpc("project/create", {
      name: "wrong target",
    }),
  ).rejects.toThrow("Connect");
  expect(requireFixtureValue(store.active()).draft).toBe("My unsent work");
});
it("preserves a running session on transport failure and reconnects without resending a prompt", async () => {
  const store = new WorkspaceStore();
  store.getSnapshot().chatDirectory = "/work";
  await store.connect();
  store.updateThread(requireFixtureValue(store.active()).id, {
    sessionId: "session",
    state: "running",
    draft: "Next instruction",
  });
  offline = true;
  await store.poll();
  expect(store.getSnapshot().connected).toBe(false);
  expect(store.active()).toMatchObject({
    state: "running",
    draft: "Next instruction",
    sessionId: "session",
  });
  offline = false;
  const now = Date.now();
  vi.spyOn(Date, "now").mockReturnValue(now + 2000);
  requests = [];
  await store.poll();
  expect(store.getSnapshot().connected).toBe(true);
  expect(store.active()).toMatchObject({
    state: "running",
    draft: "Next instruction",
    sessionId: "session",
  });
  expect(requests.some((r) => r.method === "client/sessionOpen")).toBe(true);
  expect(requests.some((r) => r.method === "session/prompt")).toBe(false);
});
function requireFixtureValue<T>(value: T | null | undefined): T {
  if (value === undefined || value === null) {
    throw new Error("Expected the fixture value to be present.");
  }
  return value;
}
