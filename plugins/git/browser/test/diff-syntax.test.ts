import { expect, test } from "bun:test";

import { codeLanguage, highlightCode } from "@swarmie/ui-web/code";

import { diffLines } from "../src/diff";
import { highlightDiff } from "../src/diff-syntax";

test("syntax preserves source and multiline comments without producing executable markup", async () => {
  const source = '/* comment\n still comment */\nexport const text = "<script>";\n';
  const tokens = await highlightCode(source, codeLanguage("src/example.ts"));
  expect(tokens.map((line) => line.map((token) => token.text).join("")).join("\n")).toBe(source);
  expect(tokens[1]?.every((token) => token.tone === "comment")).toBe(true);
  expect(tokens[2]?.some((token) => token.tone === "keyword")).toBe(true);
  expect(tokens[2]?.some((token) => token.tone === "string")).toBe(true);
  const unknown = await highlightCode("arbitrary <markup>", codeLanguage("file.unknown"));
  expect(
    unknown
      .flat()
      .map((token) => token.text)
      .join(""),
  ).toBe("arbitrary <markup>");
});

test("a context row retains independent old/new grammar state and omitted hunks reset it", async () => {
  const patch =
    '@@ -1,3 +1,2 @@\n-/*\n+export const value = 1;\n const text = "same";\n-*/\n@@ -20,1 +20,1 @@\n-export const previous = 2;\n+export const next = 3;\n';
  const lines = diffLines(patch);
  const context = lines.find((line) => line.kind === "context");
  const last = lines.findLast((line) => line.kind === "added");
  expect(context).toBeDefined();
  expect(last).toBeDefined();
  const tokens = await highlightDiff(lines, "file.ts");
  expect(tokens.get(`before:${context?.id}`)?.every((token) => token.tone === "comment")).toBe(
    true,
  );
  expect(tokens.get(`after:${context?.id}`)?.some((token) => token.tone === "keyword")).toBe(true);
  expect(tokens.get(`after:${last?.id}`)?.some((token) => token.tone === "keyword")).toBe(true);
});
