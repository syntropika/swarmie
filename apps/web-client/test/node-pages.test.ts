import { expect, test } from "bun:test";

import { Code, TembraError } from "@syntropika/tembra-sdk";

import { readNodePages } from "../src/client/node-pages";

const revision = { reason: "providers.catalog_changed", current: () => true };
function changed() {
  return new TembraError({
    code: Code.Conflict,
    reason: revision.reason,
    message: "The catalog changed.",
  });
}
async function failure(operation: Promise<unknown>) {
  try {
    await operation;
    return null;
  } catch (error: unknown) {
    return error;
  }
}

test("a changed catalog restarts its read once and discards old revision pages", async () => {
  const cursors: (string | null)[] = [];
  const pages = await readNodePages(async (cursor) => {
    cursors.push(cursor);
    if (cursor !== null) {
      throw changed();
    }
    return cursors.length === 1
      ? { value: "old", nextCursor: "old:1" }
      : { value: "new", nextCursor: null };
  }, revision);
  expect(cursors).toEqual([null, "old:1", null]);
  expect(pages).toEqual([{ value: "new", nextCursor: null }]);
});

test("a repeatedly changing catalog does not restart forever", async () => {
  const cursors: (string | null)[] = [];
  const error = await failure(
    readNodePages(async (cursor) => {
      cursors.push(cursor);
      if (cursor !== null) {
        throw changed();
      }
      return { nextCursor: "version:1" };
    }, revision),
  );
  expect(error).toBeInstanceOf(TembraError);
  expect(cursors).toEqual([null, "version:1", null, "version:1"]);
});

test("scope loss and transport failures cannot trigger a catalog restart", async () => {
  let reads = 0;
  const ended = await failure(
    readNodePages(
      async () => {
        reads += 1;
        throw changed();
      },
      { ...revision, current: () => false },
    ),
  );
  expect(ended).toBeInstanceOf(TembraError);
  expect(reads).toBe(1);
  const network = await failure(
    readNodePages(async () => {
      reads += 1;
      throw new Error("Connection gone.");
    }, revision),
  );
  expect(network).toBeInstanceOf(Error);
  expect(reads).toBe(2);
});

test("a repeated cursor is a malformed catalog and does not trigger retries", async () => {
  let reads = 0;
  const error = await failure(
    readNodePages(async () => {
      reads += 1;
      return { nextCursor: "again" };
    }, revision),
  );
  expect(error).toBeInstanceOf(Error);
  expect(reads).toBe(2);
});

test("a catalog accepts 4096 terminal pages and refuses an unbounded continuation", async () => {
  let reads = 0;
  const pages = await readNodePages(async () => {
    reads += 1;
    return { nextCursor: reads === 4096 ? null : String(reads) };
  });
  expect(pages).toHaveLength(4096);
  reads = 0;
  const error = await failure(
    readNodePages(async () => {
      reads += 1;
      return { nextCursor: String(reads) };
    }),
  );
  expect(error).toBeInstanceOf(Error);
  expect(reads).toBe(4096);
});
