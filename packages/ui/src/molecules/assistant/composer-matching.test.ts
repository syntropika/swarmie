import { describe, expect, test } from "bun:test";

import { nextComposerItem } from "./composer-matching";

describe("composer result navigation", () => {
  const items = [{ id: "first" }, { id: "disabled", disabled: true }, { id: "last" }];

  test("wraps in both directions and skips disabled results", () => {
    expect(nextComposerItem(items, "first", 1)?.id).toBe("last");
    expect(nextComposerItem(items, "last", 1)?.id).toBe("first");
    expect(nextComposerItem(items, "first", -1)?.id).toBe("last");
  });

  test("starts at the direction's edge after filtering out the selection", () => {
    expect(nextComposerItem(items, "filtered", 1)?.id).toBe("first");
    expect(nextComposerItem(items, "filtered", -1)?.id).toBe("last");
  });

  test("does not select absent or disabled results", () => {
    const emptyItems: readonly { id: string }[] = [];
    expect(nextComposerItem(emptyItems, undefined, 1)).toBeUndefined();
    expect(nextComposerItem([{ id: "only", disabled: true }], undefined, -1)).toBeUndefined();
  });
});
