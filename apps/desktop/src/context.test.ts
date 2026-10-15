import { describe, expect, it } from "vitest";

import { contextText, fileLocation, lineExcerpt, mentionAt, markdownUrl } from "~/src/context";
import { matchesShortcut, shortcutBinding } from "~/src/shortcuts";

// This URL is a rejection fixture and is never executed or assigned to the DOM.
// oxlint-disable-next-line no-script-url
const scriptUrl = "javascript:alert(1)";

describe("file context and navigation", () => {
  it("keeps colon and file-scheme links for local navigation while sanitizing other URLs", () => {
    expect(markdownUrl("hello.txt:2-3", "href")).toBe("hello.txt:2-3");
    expect(markdownUrl("file:///project/main.rs#L3", "href")).toBe("file:///project/main.rs#L3");
    expect(markdownUrl("https://example.com", "href")).toBe("https://example.com");
    expect(markdownUrl(scriptUrl, "href")).toBe("");
    expect(markdownUrl("file:///project/main.rs", "src")).toBe("");
  });
  it("preserves exact ranges and decodes file links", () => {
    expect(fileLocation("src/main.rs:12-19")).toEqual({
      path: "src/main.rs",
      startLine: 12,
      endLine: 19,
    });
    expect(fileLocation("file:///tmp/my%20project/main.rs#L3-L7")).toEqual({
      path: "/tmp/my project/main.rs",
      startLine: 3,
      endLine: 7,
    });
    expect(lineExcerpt("a\nb\nc\nd", 2, 3)).toBe("b\nc");
  });
  it("does not reinterpret external URLs as local file references", () => {
    for (const value of [
      "https://example.com/src/file.ts:3",
      scriptUrl,
      "mailto:a@b.com",
      "src/main.rs#L0",
      "src/main.rs:9-2",
    ]) {
      expect(fileLocation(value)).toBeUndefined();
    }
  });
  it("bounds ranges without silently returning unrelated context", () => {
    expect(() => lineExcerpt("a\nb", 3)).toThrow();
    expect(() => lineExcerpt("a\nb", 1, 3)).toThrow();
  });
  it("completes only the file mention before the caret", () => {
    expect(mentionAt("Read @src/mai and wait", 13)).toEqual({
      query: "src/mai",
      start: 5,
      end: 13,
    });
    expect(mentionAt("email@example.com", 17)).toBeUndefined();
    expect(mentionAt("Read @src/main.rs ", 18)).toBeUndefined();
  });
  it("retains the source kind and label with selected content", () => {
    expect(
      contextText([
        {
          id: "1",
          kind: "review",
          label: "src/main.rs · staged",
          text: "+change",
        },
      ]),
    ).toContain("Context (review): src/main.rs · staged\n+change");
  });
});
describe("custom shortcuts", () => {
  it("matches modifiers exactly so typing never triggers navigation", () => {
    const event = {
      key: "p",
      ctrlKey: true,
      metaKey: false,
      shiftKey: false,
      altKey: false,
    };
    expect(matchesShortcut(event, "Mod+P")).toBe(true);
    expect(matchesShortcut({ ...event, ctrlKey: false }, "Mod+P")).toBe(false);
    expect(matchesShortcut({ ...event, shiftKey: true }, "Mod+P")).toBe(false);
    expect(shortcutBinding({ ...event, key: "e", shiftKey: true })).toBe("Mod+Shift+E");
  });
});
