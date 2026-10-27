import { expect, test } from "bun:test";

import { spokenText } from "../src/spoken-text";

test("reads prose without Markdown syntax or code fragments", () => {
  const text = spokenText(`# A heading

A **clear** and *short* [explanation](https://example.com).

- First point
- Second point

\`\`\`typescript
console.log('never speak this');
\`\`\`

Continue here. Skip \`secret-command\` inline.

    indented_code_should_not_be_spoken()
`);
  expect(text).toContain("A heading");
  expect(text).toContain("A clear and short explanation.");
  expect(text).toContain("First point\n\nSecond point");
  expect(text).toContain("Continue here. Skip inline.");
  expect(text).not.toMatch(/console|secret-command|indented_code|https:|[*#`]/u);
});

test("reads GFM tables and checked task prose without pipes, strike marks or checkbox syntax", () => {
  const text = spokenText(
    "- [x] Completed ~~old~~ step\n\n| Name | Status |\n| --- | --- |\n| Task | Ready |\n",
  );
  expect(text).toContain("Completed old step");
  expect(text).toContain("Name. Status");
  expect(text).toContain("Task. Ready");
  expect(text).not.toMatch(/[|~]/u);
});

test("ignores reasoning, tool values and reference definitions while preserving visible link labels", () => {
  const text = spokenText([
    { type: "reasoning", text: "private reasoning" },
    { type: "tool_result", text: "private tool value" },
    {
      type: "text",
      text: "Read [the guide][guide].\n\n[guide]: https://example.com 'Hidden title'",
    },
  ]);
  expect(text).toBe("Read the guide.");
  expect(spokenText("```rust\nfn main() {}\n```\n\n`inline only`")).toBe("");
});
