import { afterEach, expect, test } from "bun:test";

import { orb } from "@syntropika/tembra-sdk";
import { createHost } from "@syntropika/tembra-sdk/node";

import { readPagedSnapshot } from "../src/connections/paging";

const cleanup: (() => Promise<void>)[] = [];
afterEach(async () => {
  await Promise.all(
    cleanup.splice(0).map(async (close) => {
      await close();
    }),
  );
});
async function fixture() {
  const host = createHost();
  cleanup.push(async () => {
    await host.close();
  });
  const value = { revision: 1, text: '🙂"\\\n'.repeat(30_000) };
  const json = JSON.stringify(value);
  const fragments = Array.from(json.matchAll(/.{1,1000}/gsu), (match) => match[0]);
  let offset = 0;
  const parts = fragments.map((data, index) => {
    const start = offset;
    offset += new TextEncoder().encode(data).byteLength;
    return {
      start,
      page: {
        revision: 1,
        data,
        next: index + 1 < fragments.length ? { revision: 1, offset } : null,
      },
    };
  });
  let current = true;
  let inconsistent = false;
  let retire = false;
  await host.install(
    orb({
      name: "fixture.pages",
      apiVersion: 1,
      services: {
        "fixture.pages": {
          create: (self) => self.state({ revision: 1 }),
          methods: {
            list: () => parts[0]?.page,
            read: () => parts[0]?.page,
            page: (_state, _context, cursor: { revision: number; offset: number }) => {
              if (retire) {
                current = false;
              }
              const found = parts.find((part) => part.start === cursor.offset);
              if (found === undefined) {
                throw new Error("Fixture cursor is invalid.");
              }
              return inconsistent ? { ...found.page, revision: 2 } : found.page;
            },
          },
        },
      },
    }),
  );
  const reference = await host.get("fixture.pages", "main");
  return {
    reference,
    value,
    current: () => current,
    retire: () => {
      retire = true;
    },
    inconsistent: () => {
      inconsistent = true;
    },
  };
}

test("paged projections assemble one huge escaped Unicode item through actual SDK references", async () => {
  const setup = await fixture();
  const value = await readPagedSnapshot(setup.reference, setup.current);
  expect(value).toEqual(setup.value);
  expect(await readPagedSnapshot(setup.reference, setup.current, 0, "read")).toEqual(setup.value);
  setup.reference.release();
});

test("retired sessions and mixed revisions discard incomplete page sets", async () => {
  const first = await fixture();
  first.retire();
  expect(await failureOf(readPagedSnapshot(first.reference, first.current))).toBeInstanceOf(Error);
  first.reference.release();
  const second = await fixture();
  second.inconsistent();
  expect(await failureOf(readPagedSnapshot(second.reference, second.current))).toBeInstanceOf(
    Error,
  );
  second.reference.release();
});

async function failureOf(operation: Promise<unknown>) {
  try {
    await operation;
    return null;
  } catch (error: unknown) {
    return error;
  }
}
