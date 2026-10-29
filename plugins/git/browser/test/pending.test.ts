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
