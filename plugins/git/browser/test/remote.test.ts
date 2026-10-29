import { afterEach, expect, test } from "bun:test";

import { orb } from "@syntropika/tembra-sdk";
import { createHost } from "@syntropika/tembra-sdk/node";

import type { RepositoryTarget } from "../src/protocol";
import { GitRemote } from "../src/remote";
import type { GitConnection } from "../src/remote";

const target: RepositoryTarget = {
  root: "/checkout",
  git_dir: "/checkout/.git",
  common_dir: "/checkout/.git",
  bare: false,
  head: "abc",
  branch: null,
  fingerprint: "captured",
  index_fingerprint: "index",
  observation_error: null,
};

const cleanup: (() => Promise<void>)[] = [];
afterEach(async () => {
  await Promise.all(
    cleanup.splice(0).map(async (close) => {
      await close();
    }),
  );
});

async function fixture(request: () => unknown) {
  const host = createHost();
  await host.install(
    orb({
      name: "fixture.git",
      apiVersion: 1,
      services: {
        "swarmie.git": { create: () => undefined, methods: { request } },
      },
    }),
  );
  cleanup.push(async () => {
    await host.close();
  });
  let current = true;
  let acquired = 0;
  const connection: GitConnection = {
    nodeId: "node-a",
    principal: "alice",
    generation: 7,
    isCurrent: () => current,
    get: async (service, key) => {
      acquired += 1;
      expect(service).toBe("swarmie.git");
      expect(key).toBe("main");
      return await host.get(service, key);
    },
  };
  return {
    remote: new GitRemote(connection),
    end: () => {
      current = false;
    },
    acquired: () => acquired,
  };
}

test("Git references remain on the captured node session", async () => {
  const state = await fixture(() => ({ kind: "repository", repository: null }));
  expect(await state.remote.request({ kind: "discover", path: "/checkout" })).toEqual({
    kind: "repository",
    repository: null,
  });
  expect(state.remote.identity).toEqual({ nodeId: "node-a", principal: "alice", generation: 7 });
  state.end();
  expect(await rejected(state.remote.request({ kind: "discover", path: "/checkout" }))).toContain(
    "session ended",
  );
  expect(state.acquired()).toBe(1);
});

test("a response from an ended session cannot become visible or authorize a subsequent operation", async () => {
  const waiting = Promise.withResolvers<unknown>();
  const dispatched = Promise.withResolvers<undefined>();
  const state = await fixture(async () => {
    dispatched.resolve(undefined);
    return await waiting.promise;
  });
  const response = state.remote.request({ kind: "discover", path: "/checkout" });
  await dispatched.promise;
  state.end();
  waiting.resolve({ kind: "repository", repository: null });
  expect(await rejected(response)).toContain("session ended");
});

test("native response validation runs before values reach the view", async () => {
  const state = await fixture(() => ({ kind: "file", text: 17, truncated: false }));
  expect(await rejected(state.remote.request({ kind: "discover", path: "/checkout" }))).toContain(
    "invalid response",
  );
});

test("an operation receipt from another principal cannot settle this user's pending effect", async () => {
  const state = await fixture(() => ({
    kind: "receipt",
    receipt: {
      operation_id: "operation-a",
      principal: "bob",
      target,
      state: "completed",
      message: "done",
      head: "abc",
    },
  }));
  expect(
    await rejected(state.remote.request({ kind: "recover", operation_id: "operation-a" })),
  ).toContain("another operation or principal");
});

test("status responses cannot silently replace the captured repository", async () => {
  const state = await fixture(() => ({
    kind: "status",
    next_page: null,
    status: { target: { ...target, root: "/another-checkout" }, entries: [] },
  }));
  expect(await rejected(state.remote.request({ kind: "status", target }))).toContain(
    "different checkout",
  );
});

test("a compact unknown managed receipt is preserved without inventing a created worktree", async () => {
  const state = await fixture(() => ({
    kind: "receipt",
    receipt: {
      operation_id: "managed-a",
      principal: "alice",
      target,
      state: "unknown",
      message: "Reconciliation required",
      head: "abc",
    },
  }));
  const response = await state.remote.request({
    kind: "worktree_create",
    operation_id: "managed-a",
    target,
    request: {
      managed_root: "/managed",
      destination: "/managed/new",
      commit: "abc",
      include_paths: [],
    },
  });
  expect(response.kind).toBe("receipt");
  if (response.kind === "receipt") {
    expect(response.receipt.state).toBe("unknown");
  }
});

test("recovery discovery cannot mix another principal or repository into the catalog", async () => {
  await Promise.all(
    [
      { principal: "bob", target },
      { principal: "alice", target: { ...target, common_dir: "/other/.git" } },
    ].map(async (receipt) => {
      const state = await fixture(() => ({
        kind: "operations",
        operations: [
          {
            ...receipt,
            operation_id: "foreign",
            state: "unknown",
            message: "interrupted",
            head: "abc",
          },
        ],
        unavailable: [],
        next_page: null,
      }));
      expect(await rejected(state.remote.request({ kind: "operations", target }))).toContain(
        "another principal or repository",
      );
    }),
  );
});

async function rejected(pending: Promise<unknown>) {
  try {
    await pending;
    return "Unexpected success";
  } catch (error: unknown) {
    return error instanceof Error ? error.message : String(error);
  }
}

test("a reference acquisition failure retains a captured request that can be fenced after reconnect", async () => {
  const { pendingKey, pendingRecovery, readPending, writePending } = await import("../src/pending");
  const failedConnection: GitConnection = {
    nodeId: "node-a",
    principal: "alice",
    generation: 1,
    isCurrent: () => true,
    get: async () => {
      throw new Error("worker unavailable before dispatch");
    },
  };
  const key = pendingKey(failedConnection, "/reference-failure");
  writePending(key, "never-admitted", target);
  const failed = new GitRemote(failedConnection);
  expect(
    await rejected(
      failed.request({
        kind: "mutate",
        operation_id: "never-admitted",
        target,
        operation: { kind: "stage", paths: ["file.txt"] },
      }),
    ),
  ).toContain("before dispatch");
  expect(readPending(key)).toBe("never-admitted");
  const reconnected = await fixture(() => ({
    kind: "receipt",
    receipt: {
      operation_id: "never-admitted",
      principal: "alice",
      target,
      state: "failed",
      message: "Durable fence confirmed",
      head: target.head,
    },
  }));
  const reconciliation = pendingRecovery(key, "never-admitted");
  expect(reconciliation.kind).toBe("reconcile");
  const result = await reconnected.remote.request(reconciliation);
  expect(result.kind === "receipt" && result.receipt.state).toBe("failed");
  // The view clears its marker only after this validated terminal reply.
  writePending(key, null);
  expect(readPending(key)).toBeNull();
});
