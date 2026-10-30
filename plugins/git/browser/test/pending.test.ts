import { expect, test } from "bun:test";

import { pendingKey, readPending, writePending } from "../src/pending";
import type { GitConnection } from "../src/remote";

function connection(principal: string, generation: number): GitConnection {
  return {
    nodeId: "node-a",
    principal,
    generation,
    isCurrent: () => true,
    get: async () => {
      throw new Error("No capability is acquired by this fixture.");
    },
  };
}

test("reconnection retains pending operation identity while another principal cannot inherit it", () => {
  const original = pendingKey(connection("alice", 1), "/checkout");
  const reconnected = pendingKey(connection("alice", 2), "/checkout");
  const another = pendingKey(connection("bob", 2), "/checkout");
  writePending(original, "operation-unknown");
  expect(readPending(reconnected)).toBe("operation-unknown");
  expect(readPending(another)).toBeNull();
  writePending(reconnected, null);
  expect(readPending(original)).toBeNull();
});

test("pending identity survives refresh without inheriting another checkout observation", async () => {
  const { pendingRecovery } = await import("../src/pending");
  const target = {
    root: "/original",
    git_dir: "/original/.git",
    common_dir: "/original/.git",
    bare: false,
    head: "first",
    branch: "main",
    fingerprint: "observed",
    index_fingerprint: "index",
    observation_error: null,
  };
  const key = pendingKey(connection("pending-refresh", 1), "/original");
  writePending(key, "original", target);
  target.head = "changed-by-caller";
  writePending(key, "original", { ...target, root: "/another" });
  const request = pendingRecovery(key, "original");
  expect(request.kind).toBe("reconcile");
  expect("target" in request && request.target.root).toBe("/original");
  expect("target" in request && request.target.head).toBe("first");
  expect(() => pendingRecovery(key, "another")).toThrow("changed");
  writePending(key, null);
});

test("a retained opaque marker can capture a verified recovery target without losing its ID", async () => {
  const { pendingRecovery } = await import("../src/pending");
  const key = pendingKey(connection("pending-opaque", 1), "/checkout");
  writePending(key, "retained-id");
  expect(pendingRecovery(key, "retained-id").kind).toBe("recover");
  writePending(key, "retained-id", {
    root: "/checkout",
    git_dir: "/checkout/.git",
    common_dir: "/checkout/.git",
    bare: false,
    head: null,
    branch: "main",
    fingerprint: "observed",
    index_fingerprint: "index",
    observation_error: null,
  });
  expect(pendingRecovery(key, "retained-id").kind).toBe("reconcile");
  expect(readPending(key)).toBe("retained-id");
  writePending(key, null);
});
