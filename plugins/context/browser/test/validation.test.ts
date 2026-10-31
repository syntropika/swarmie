import { expect, test } from "bun:test";

import { compactAlias, contextView, receipt } from "../src/validation";

test("malformed Context data and a mismatched admission receipt cannot become usable UI", () => {
  expect(() =>
    contextView({ checkpoint: {}, inspection: null, can_compact: true, unavailable: null }),
  ).toThrow();
  expect(() => receipt({ requested: "other" }, "expected")).toThrow();
  expect(compactAlias([{ id: "swarmie.context.compact", aliases: ["/condense"] }])).toBe(
    "condense",
  );
  expect(() => compactAlias([{ id: "swarmie.other", aliases: ["/compact"] }])).toThrow();
});
