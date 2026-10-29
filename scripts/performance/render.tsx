import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { CodePreview } from "../../packages/ui/web/src/code";
import { diffLines } from "../../plugins/git/browser/src/diff";
import { highlightDiff } from "../../plugins/git/browser/src/diff-syntax";
import { DiffView } from "../../plugins/git/browser/src/diff-view";

const [operation, sizeArgument] = process.argv.slice(2);
const size = Number(sizeArgument);
if (!Number.isSafeInteger(size) || size < 1 || size > 5000) {
  throw new Error("Invalid render fixture size.");
}
const patch = `@@ -0,0 +1,${size} @@\n${"+x\n".repeat(size)}`;
const started = performance.now();
if (operation === "syntax") {
  const before = Array.from({ length: size }, (_, index) => `-const value${index} = "old";`).join(
    "\n",
  );
  const after = Array.from({ length: size }, (_, index) => `+const value${index} = "new";`).join(
    "\n",
  );
  const tokens = await highlightDiff(
    diffLines(`@@ -1,${size} +1,${size} @@\n${before}\n${after}\n`),
    "fixture.ts",
  );
  if (tokens.size !== size * 2) {
    throw new Error("Incomplete syntax fixture.");
  }
  console.log(
    JSON.stringify({
      elapsed_ms: performance.now() - started,
      highlighted_lines: tokens.size,
      verified_items: { lines: size * 2 },
    }),
  );
} else {
  if (operation !== "preview" && operation !== "unified" && operation !== "split") {
    throw new Error("Unknown render case.");
  }
  const element =
    operation === "preview"
      ? createElement(CodePreview, { code: "x\n".repeat(size), path: "fixture.ts" })
      : createElement(DiffView, {
          text: patch,
          mode: operation,
          path: "fixture.ts",
          staged: false,
        });
  const first = renderToStaticMarkup(element);
  const coldRenderMs = performance.now() - started;
  let html = first;
  const repeated = performance.now();
  for (let iteration = 0; iteration < 5; iteration += 1) {
    html = renderToStaticMarkup(element);
  }
  const elapsed = (performance.now() - repeated) / 5;
  if (!html.includes(operation === "preview" ? 'aria-label="File content"' : "data-git-diff")) {
    throw new Error("Missing rendered fixture root.");
  }
  console.log(
    JSON.stringify({
      elapsed_ms: elapsed,
      cold_render_ms: coldRenderMs,
      html_bytes: Buffer.byteLength(html),
      rendered_rows:
        operation === "preview"
          ? (html.match(/class="flex min-w-max"/gu)?.length ?? 0)
          : (html.match(/<tr/gu)?.length ?? 0),
      verified_items: {
        measured_renders: 5,
        input_lines: size,
        input_bytes: Buffer.byteLength(operation === "preview" ? "x\n".repeat(size) : patch),
      },
    }),
  );
}
