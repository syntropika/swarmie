import { expect, it } from "vitest";

import { fingerprint, reviewKey } from "./reviewed";

it("reviewed marks distinguish scopes and invalidate when file content changes", async () => {
  expect(reviewKey("tree", "base", "main", "file.txt")).not.toBe(
    reviewKey("tree", "base", "other", "file.txt"),
  );
  expect(await fingerprint("old diff")).not.toBe(await fingerprint("new diff"));
  expect(await fingerprint("stable")).toBe(await fingerprint("stable"));
});
