import { describe, expect, test } from "bun:test";

import { renderToStaticMarkup } from "react-dom/server";

import { FileDiff } from "./file-diff";
import { InlineCitations } from "./inline-citations";

describe("AICSS output boundaries", () => {
  test("citations leave unresolved markers readable and reject executable source URLs", () => {
    const html = renderToStaticMarkup(
      <InlineCitations
        text="Evidence[1], unknown[2], invalid[3]."
        sourcesLabel="Sources"
        references={[
          {
            number: 1,
            label: "Verified source",
            host: "example.com",
            url: "https://example.com/source",
          },
          {
            number: 3,
            label: "Executable source",
            host: "invalid",
            url: ["javascript", "alert(1)"].join(":"),
          },
        ]}
      />,
    );
    expect(html).toContain('href="https://example.com/source"');
    expect(html).toContain("[2]");
    expect(html).toContain("[3]");
    expect(html).not.toContain('href="javascript:');
  });

  test("diff source is escaped rather than interpreted as markup", () => {
    const html = renderToStaticMarkup(
      <FileDiff
        label="Untrusted diff"
        describeRow={(row) => `${row.kind}, line ${row.newLine ?? row.oldLine}`}
        file="view.html"
        language="unknown"
        rows={[
          {
            id: "change",
            kind: "added",
            oldLine: null,
            newLine: 1,
            text: '<script>alert("x")</script>',
          },
        ]}
      />,
    );
    expect(html).not.toContain("<script>");
    expect(html).toContain("&lt;script&gt;");
    expect(html).toContain("+1");
    expect(html).toContain("−0");
  });
});
