import { expect, test } from "bun:test";

import { diffLines, splitDiff } from "../src/diff";
import { responseValue, settingsValue } from "../src/protocol";

test("diff display preserves zero-length ranges, additions, removals and aligned context", () => {
  const lines = diffLines(
    "diff --git a/x b/x\n@@ -0,0 +1,2 @@\n+first\n+second\n@@ -5,2 +7,2 @@\n-before\n+after\n context\n",
  );
  expect(lines.filter((line) => line.kind === "added").map((line) => line.newLine)).toEqual([
    1, 2, 7,
  ]);
  expect(lines.find((line) => line.kind === "removed")?.oldLine).toBe(5);
  const rows = splitDiff(lines);
  const changed = rows.find((row) => row.left?.text === "before");
  expect(changed?.right?.text).toBe("after");
  expect(rows.find((row) => row.left?.text === "context")?.right?.newLine).toBe(8);
});

test("untrusted protocol values cannot coerce paths, booleans or arrays", () => {
  expect(() => responseValue({ kind: "files", paths: ["valid", 3] })).toThrow("invalid response");
  expect(() => responseValue({ kind: "file", text: "text", truncated: "false" })).toThrow(
    "invalid response",
  );
  expect(() => responseValue({ kind: "history", commits: {} })).toThrow("invalid response");
  expect(() => responseValue({ kind: "not-supported" })).toThrow("invalid response");
});

test("hook preferences require an explicit boolean from the node", () => {
  const settings = {
    diff_layout: "unified",
    include_local_changes: false,
    managed_root: null,
    branch_prefix: "",
    commit_guidance: "",
    run_commit_hooks: false,
    cleanup_enabled: false,
    retained_worktrees: null,
    setup_by_platform: {},
  };
  expect(settingsValue(settings).run_commit_hooks).toBe(false);
  expect(settingsValue({ ...settings, run_commit_hooks: true }).run_commit_hooks).toBe(true);
  expect(() => settingsValue({ ...settings, run_commit_hooks: "false" })).toThrow(
    "invalid response",
  );
});
