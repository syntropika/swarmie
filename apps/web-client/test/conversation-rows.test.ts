import { expect, test } from "bun:test";

import type { Message } from "@swarmie/plugin-conversations-browser";

import { transcriptRows } from "../src/conversations/rows";

const messages: readonly Message[] = Object.freeze([
  Object.freeze({ id: "user", role: "user", text: "Check the files" }),
  Object.freeze({ id: "first", role: "assistant", text: "I will inspect them." }),
  Object.freeze({ id: "tool-a", role: "tool", text: "First result" }),
  Object.freeze({ id: "tool-b", role: "tool", text: "Second result", truncated: true }),
  Object.freeze({ id: "second", role: "assistant", text: "Here is the answer." }),
]);

test("grouping only combines adjacent tool results and preserves canonical records", () => {
  const before = JSON.stringify(messages);
  const rows = transcriptRows(messages, "grouped", true);
  expect(rows.map((row) => row.key)).toEqual([
    "message:user",
    "message:first",
    "tools:tool-a",
    "message:second",
    "activity",
  ]);
  const group = rows.find((row) => row.kind === "tools");
  expect(group?.kind).toBe("tools");
  if (group?.kind !== "tools") {
    throw new Error("Expected a result group.");
  }
  expect(group.messages).toEqual(messages.slice(2, 4));
  expect(JSON.stringify(messages)).toBe(before);
});

test("hidden results leave every assistant text in its own chronological row", () => {
  const rows = transcriptRows(messages, "hidden", true);
  expect(rows.map((row) => row.key)).toEqual([
    "message:user",
    "message:first",
    "message:second",
    "activity",
  ]);
  expect(rows.some((row) => row.kind === "tools")).toBe(false);
});

test("separate results each remain available without claiming a call or success state", () => {
  const rows = transcriptRows(messages, "separate", false);
  expect(rows.filter((row) => row.kind === "tools").map((row) => row.messages.length)).toEqual([
    1, 1,
  ]);
  expect(rows.at(-1)?.key).toBe("message:second");
});

test("a loaded window can start with orphan tool results and keeps plugin messages", () => {
  const orphan = messages.find((message) => message.id === "tool-a");
  if (orphan === undefined) {
    throw new Error("The result fixture is missing.");
  }
  const plugin: Message = { id: "plugin", role: "assistant", text: "", kind: "fixture.view" };
  const rows = transcriptRows(
    [
      orphan,
      { id: "empty", role: "assistant", text: "" },
      plugin,
      { id: "activity", role: "user", text: "A distinct identity" },
    ],
    "grouped",
    true,
  );
  expect(rows.map((row) => row.key)).toEqual([
    "tools:tool-a",
    "message:plugin",
    "message:activity",
    "activity",
  ]);
});
