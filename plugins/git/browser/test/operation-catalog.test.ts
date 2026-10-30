import { expect, test } from "bun:test";

import { readArchives, readOperations } from "../src/operation-catalog";
import { responseValue } from "../src/protocol";
import type { ArchiveReceipt, GitReceipt, RepositoryTarget } from "../src/protocol";

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
const receipt: GitReceipt = {
  operation_id: "interrupted",
  principal: "alice",
  target,
  state: "unknown",
  message: "Inspect before retrying",
  head: "abc",
};

test("durable operation discovery includes interrupted and running effects without replay", async () => {
  const calls: string[] = [];
  const result = await readOperations(
    {
      request: async (request) => {
        calls.push(request.kind);
        if (request.kind !== "operations") {
          throw new Error("No mutation is allowed during discovery.");
        }
        return request.page === undefined
          ? {
              kind: "operations",
              operations: [receipt],
              unavailable: [],
              next_page: { offset: 1, digest: "catalog" },
            }
          : {
              kind: "operations",
              operations: [{ ...receipt, operation_id: "running", state: "in_progress" }],
              unavailable: [{ entry_id: "opaque", message: "A recovery record is unreadable." }],
              next_page: null,
            };
      },
    },
    target,
  );
  expect(result.operations.map((operation) => operation.state)).toEqual(["unknown", "in_progress"]);
  expect(result.unavailable).toHaveLength(1);
  expect(calls).toEqual(["operations", "operations"]);
});

test("changed or skipping operation catalogs cannot publish a partial recovery set", async () => {
  await Promise.all(
    [
      { offset: 2, digest: "catalog" },
      { offset: 1, digest: "changed" },
    ].map(async (next) => {
      let calls = 0;
      let failure: unknown;
      try {
        await readOperations(
          {
            request: async () => {
              calls += 1;
              return {
                kind: "operations",
                operations: [receipt],
                unavailable: [],
                next_page: calls === 1 ? { offset: 1, digest: "catalog" } : next,
              };
            },
          },
          target,
        );
      } catch (error: unknown) {
        failure = error;
      }
      expect(failure).toBeInstanceOf(Error);
      expect(failure instanceof Error ? failure.message : "").toContain("changed during recovery");
    }),
  );
});

test("archive listing distinguishes missing evidence from an empty archive catalog", () => {
  expect(
    responseValue({
      kind: "archives",
      next_page: null,
      archives: [],
      unavailable: [{ entry_id: "opaque", message: "Snapshot could not be read." }],
    }),
  ).toEqual({
    kind: "archives",
    next_page: null,
    archives: [],
    unavailable: [{ entry_id: "opaque", message: "Snapshot could not be read." }],
  });
  expect(() =>
    responseValue({
      kind: "archives",
      next_page: null,
      archives: [],
      unavailable: [{ entry_id: 5, message: "invalid" }],
    }),
  ).toThrow("invalid response");
  expect(() => responseValue({ kind: "archives", archives: [] })).toThrow("invalid response");
});

const archive: ArchiveReceipt = {
  archive_id: "saved",
  target,
  head: "abc",
  files: 2,
  restore_verified: false,
  restored_target: null,
  cleanup_blocked_reason: "Writers must stop first.",
};

test("archive pagination counts snapshots and unreadable entries in one stable inventory", async () => {
  const requestedOffsets: number[] = [];
  const warnings = Array.from({ length: 1000 }, (_, index) => ({
    entry_id: `opaque-${index}`,
    message: "Unreadable recovery record. ".repeat(500),
  }));
  const result = await readArchives(
    {
      request: async (request) => {
        if (request.kind !== "archive_list") {
          throw new Error("Only archive listing is allowed.");
        }
        const offset = request.page?.offset ?? 0;
        requestedOffsets.push(offset);
        if (offset === 0) {
          return {
            kind: "archives",
            archives: [archive],
            unavailable: warnings.slice(0, 1),
            next_page: { offset: 2, digest: "stable" },
          };
        }
        const end = Math.min(offset + 2, warnings.length + 1);
        return {
          kind: "archives",
          archives: [],
          unavailable: warnings.slice(offset - 1, end - 1),
          next_page: end === warnings.length + 1 ? null : { offset: end, digest: "stable" },
        };
      },
    },
    target,
  );
  expect(result.archives).toEqual([archive]);
  expect(result.unavailable).toEqual(warnings);
  expect(requestedOffsets.slice(0, 3)).toEqual([0, 2, 4]);
  expect(requestedOffsets.at(-1)).toBe(1000);
});

test("invalid archive continuation or failed final page never returns a partial catalog", async () => {
  await Promise.all(
    ["changed", "skipping", "stalled", "final-failure"].map(async (scenario) => {
      let calls = 0;
      let failure: unknown;
      let published = false;
      try {
        await readArchives(
          {
            request: async () => {
              calls += 1;
              if (calls === 1) {
                return {
                  kind: "archives",
                  archives: [archive],
                  unavailable: [{ entry_id: "unreadable", message: "Record unavailable." }],
                  next_page: { offset: 2, digest: "stable" },
                };
              }
              if (scenario === "final-failure") {
                throw new Error("The final archive page failed.");
              }
              const next =
                scenario === "changed"
                  ? { offset: 3, digest: "changed" }
                  : { offset: scenario === "stalled" ? 2 : 4, digest: "stable" };
              return {
                kind: "archives",
                archives: [],
                unavailable: [{ entry_id: "next", message: "Another unavailable record." }],
                next_page: next,
              };
            },
          },
          target,
        );
        published = true;
      } catch (error: unknown) {
        failure = error;
      }
      expect(published).toBe(false);
      expect(failure).toBeInstanceOf(Error);
      expect(calls).toBe(2);
    }),
  );
});
