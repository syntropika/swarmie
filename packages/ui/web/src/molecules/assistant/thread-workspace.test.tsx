import { describe, expect, test } from "bun:test";

import { renderToStaticMarkup } from "react-dom/server";

import { ThreadWorkspaceExample } from "./examples/thread-workspace";
import { StreamingText } from "./streaming-text";
import { groupThreadParts } from "./thread-workspace";
import type { ThreadMessagePart } from "./thread-workspace";
import { clampWorkspaceSize, parseWorkspaceSize } from "./thread-workspace-size";

const tool = (id: string, standalone = false): ThreadMessagePart => ({
  id,
  kind: "tool",
  standalone,
  tool: {
    name: id,
    activeLabel: id,
    state: "completed",
    statusLabel: "Completed",
    request: "Request",
    requestLabel: "Request",
    resultLabel: "Result",
  },
});

describe("assembled thread boundaries", () => {
  test("groups adjacent tools without moving standalone tools or text", () => {
    const parts: ThreadMessagePart[] = [
      tool("one"),
      tool("two"),
      { id: "text", kind: "text", text: "Result", format: "plain" },
      tool("three", true),
      tool("four"),
    ];
    const groups = groupThreadParts(parts);
    expect(groups.map((group) => group.kind)).toEqual(["tools", "single", "single", "tools"]);
    expect(groups[0]).toMatchObject({ parts: [{ id: "one" }, { id: "two" }] });
    expect(parts).toHaveLength(5);
  });
  test("groups contiguous reasoning without mutating supplied sentences", () => {
    const reasoning = {
      sentences: [{ id: "step", text: "Inspect" }],
      phase: "completed" as const,
      thinkingLabel: "Thinking",
      completedLabel: "Reasoning",
      toggleLabel: "Toggle",
    };
    const groups = groupThreadParts([
      { id: "first", kind: "reasoning", reasoning },
      { id: "second", kind: "reasoning", reasoning },
    ]);
    expect(groups).toHaveLength(1);
    expect(groups[0]).toMatchObject({
      kind: "reasoning",
      parts: [{ id: "first" }, { id: "second" }],
    });
    expect(reasoning.sentences).toHaveLength(1);
  });
  test("whole-text segments still reveal only the requested words", () => {
    const html = renderToStaticMarkup(
      <StreamingText segments={[{ text: "Checking focus labels" }]} count={1} streaming />,
    );
    expect(html).toContain("Checking");
    expect(html).not.toContain("focus");
    expect(html).not.toContain("labels");
  });
  test("floating workspace is SSR safe and closed until its host opens it", () => {
    const html = renderToStaticMarkup(<ThreadWorkspaceExample />);
    expect(html).toContain("Open assistant workspace");
    expect(html).toContain("<dialog");
    expect(html).not.toContain("<dialog open");
    expect(html).toContain("Read component contract");
  });
});

describe("floating workspace geometry", () => {
  test("clamps desktop and narrow viewport extents without overflowing", () => {
    expect(clampWorkspaceSize({ width: 900, height: 900 }, { width: 800, height: 700 })).toEqual({
      width: 768,
      height: 604,
    });
    expect(clampWorkspaceSize({ width: 1, height: 1 }, { width: 1000, height: 900 })).toEqual({
      width: 320,
      height: 400,
    });
    expect(clampWorkspaceSize({ width: 400, height: 500 }, { width: 300, height: 400 })).toEqual({
      width: 268,
      height: 304,
    });
  });
  test("rejects malformed and non-finite persisted dimensions", () => {
    for (const raw of [
      null,
      "broken",
      "null",
      "[]",
      '{"width":"400","height":500}',
      '{"width":1e999,"height":500}',
    ]) {
      expect(parseWorkspaceSize(raw)).toBeUndefined();
    }
    expect(parseWorkspaceSize('{"width":440,"height":560,"extra":true}')).toEqual({
      width: 440,
      height: 560,
    });
  });
});
