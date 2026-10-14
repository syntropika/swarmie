import { describe, expect, test } from "bun:test";

import { renderToStaticMarkup } from "react-dom/server";

import { MarkdownText, MathBlock, SyntaxHighlighter } from "./renderers";
import { WebPreview } from "./structured";

describe("assistant renderer boundaries", () => {
  test("markdown keeps formatted content but excludes raw HTML and executable link protocols", () => {
    const html = renderToStaticMarkup(
      <MarkdownText
        text={
          "**Evidence**\n\n<script>alert(1)</script>\n\n[unsafe](javascript:alert(1))\n\n[documentation](https://example.com/docs)"
        }
      />,
    );
    expect(html).toContain("<strong>Evidence</strong>");
    expect(html).not.toContain("<script");
    expect(html).not.toContain('href="javascript:');
    expect(html).toContain('href="https://example.com/docs"');
  });

  test("GFM tables and task lists retain their semantic structure", () => {
    const html = renderToStaticMarkup(
      <MarkdownText
        text={"| Item | State |\n| --- | --- |\n| Review | Ready |\n\n- [x] Inspect the result"}
      />,
    );
    expect(html).toContain("<table>");
    expect(html).toContain("<th>Item</th>");
    expect(html).toContain('type="checkbox"');
    expect(html).toContain("disabled");
  });

  test("unsupported code grammars preserve source as escaped text", () => {
    const source = '<script>fetch("/private")</script>';
    const html = renderToStaticMarkup(
      <SyntaxHighlighter source={source} language="unknown-grammar" label="Untrusted code" />,
    );
    expect(html).not.toContain("<script>");
    expect(html).toContain("&lt;script&gt;");
    expect(html).toContain("Untrusted code");
  });

  test("math retains MathML and rejects trusted URL commands", () => {
    const expression = "\\href{javascript:alert(1)}{x} + \\frac{1}{2}";
    const html = renderToStaticMarkup(
      <MathBlock expression={expression} label="Untrusted formula" />,
    );
    expect(html).toContain("<math");
    expect(html).toContain("<mfrac>");
    expect(html).not.toContain('href="javascript:');
  });

  test("preview navigation cannot enable script execution or same-origin access", () => {
    const html = renderToStaticMarkup(
      <WebPreview
        src="https://example.com"
        label="Generated page"
        reloadLabel="Reload"
        openLabel="Open"
      />,
    );
    expect(html).toContain('sandbox=""');
    expect(html).toContain('referrerPolicy="no-referrer"');
    expect(html).not.toContain("allow-scripts");
    expect(html).not.toContain("allow-same-origin");
  });
});

test("registered source grammars highlight TypeScript, TSX, JSX and Python", () => {
  for (const [language, source] of [
    ["ts", "const count: number = 2;"],
    ["tsx", "const view = <span />;"],
    ["jsx", "const view = <span />;"],
    ["python", "def count():\n  return 2"],
  ]) {
    const html = renderToStaticMarkup(
      <SyntaxHighlighter language={language ?? ""} source={source ?? ""} label="Source" />,
    );
    expect(html).toContain("text-inverse-link");
  }
});

test("revealed math steps render MathML and markdown code exposes copy controls", () => {
  const html = renderToStaticMarkup(
    <MathBlock
      label="Derivation"
      expression="x"
      visibleSteps={1}
      steps={[
        { id: "one", expression: "\\frac{1}{2}", explanation: "Half" },
        { id: "two", expression: "x=4", explanation: "Hidden step" },
      ]}
    />,
  );
  expect(html).toContain("<mfrac>");
  expect(html).not.toContain("Hidden step");
  const markdown = renderToStaticMarkup(<MarkdownText text={"```ts\nconst value = 1;\n```"} />);
  expect(markdown).toContain('aria-label="Copy code"');
  expect(markdown).toContain(">ts</span>");
});
