import { expect, test } from "bun:test";

import { renderToStaticMarkup } from "react-dom/server";

import { ReasoningEffort } from "./reasoning-effort";

test("reasoning uses the consumer's exact supported values and accessible labels", () => {
  const markup = renderToStaticMarkup(
    <ReasoningEffort
      levels={[
        { id: "balanced", label: "Balanced" },
        { id: "deep", label: "Deep" },
      ]}
      value="deep"
      onChange={() => undefined}
    />,
  );
  expect(markup).toContain('class="sr-only">: Deep</span>');
  expect(markup).toContain('value="1"');
  expect(markup).toContain('max="1"');
  expect(markup).not.toContain("Extra high");
  expect(markup).not.toContain("disabled=");
});

test("unavailable and single-level inputs cannot imply an adjustable budget", () => {
  const empty = renderToStaticMarkup(
    <ReasoningEffort levels={[]} value="unknown" onChange={() => undefined} />,
  );
  expect(empty).toContain("Unavailable");
  expect(empty).not.toContain('type="range"');
  for (const value of ["high", "unknown"]) {
    const markup = renderToStaticMarkup(
      <ReasoningEffort
        levels={[{ id: "high", label: "High" }]}
        value={value}
        onChange={() => undefined}
      />,
    );
    expect(markup).toContain('disabled=""');
    expect(markup).toContain(
      `class="sr-only">: ${value === "high" ? "High" : "Unavailable"}</span>`,
    );
  }
});
