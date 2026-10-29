import { expect, test } from "bun:test";

import { renderToStaticMarkup } from "react-dom/server";

import { ReasoningIndicator } from "./reasoning-indicator";

test("the same named effort follows each model's own ordered scale", () => {
  const levels = ["brief", "balanced", "deep"];
  const model = renderToStaticMarkup(<ReasoningIndicator levels={levels} value="balanced" />);
  const other = renderToStaticMarkup(
    <ReasoningIndicator levels={["balanced", "deep"]} value="balanced" />,
  );
  expect(model).toContain('stroke-dasharray="66.66666666666666 100"');
  expect(other).toContain('stroke-dasharray="50 100"');
  expect(model).toContain('aria-hidden="true"');
});

test("an unknown or disabled effort never paints a misleading filled ring", () => {
  for (const [levels, value] of [
    [[], "deep"],
    [["brief"], "missing"],
    [["off", "deep"], "off"],
  ] as const) {
    const markup = renderToStaticMarkup(<ReasoningIndicator levels={levels} value={value} />);
    expect(markup).not.toContain("stroke-dasharray");
  }
});
