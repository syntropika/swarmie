import { expect, test } from "bun:test";

import { changeGroups, compactSelection } from "../src/file-selection";
import type { StatusEntry } from "../src/protocol";

const changed: StatusEntry = {
  path: "src/example.ts",
  original_path: null,
  index: ".",
  worktree: "M",
  kind: "tracked",
};

test("mobile review follows the same file across staging and unstaging", () => {
  const selected = { path: changed.path, staged: false, source: "diff" as const };
  const staged = { ...changed, index: "M", worktree: "." };
  const next = compactSelection([staged], "all", selected);
  expect(next).toEqual({ ...selected, staged: true });
  expect(compactSelection([changed], "all", next)).toEqual(selected);
  expect(compactSelection([{ ...staged, worktree: "M" }], "all", selected)).toEqual(selected);
});

test("filters and empty checkouts never retain a stale file review", () => {
  const staged = { ...changed, path: "src/staged.ts", index: "M", worktree: "." };
  const selected = { path: changed.path, staged: false, source: "diff" as const };
  expect(compactSelection([changed, staged], "staged", selected)?.path).toBe(staged.path);
  expect(compactSelection([changed], "staged", selected)).toBeNull();
  expect(compactSelection([], "all", selected)).toBeNull();
});

test("untracked files become index diffs when staged; ignored paths stay out of review", () => {
  const untracked: StatusEntry = { ...changed, kind: "untracked", index: "?", worktree: "?" };
  const ignored: StatusEntry = { ...untracked, path: "private.env", kind: "ignored" };
  const selected = compactSelection([ignored, untracked], "all", null);
  expect(selected).toEqual({ path: changed.path, staged: false, source: "file" });
  expect(compactSelection([{ ...changed, index: "A", worktree: "." }], "all", selected)).toEqual({
    path: changed.path,
    staged: true,
    source: "diff",
  });
  expect(changeGroups([ignored], "all").flatMap((group) => group.entries)).toEqual([]);
});
