import { describe, expect, test } from "bun:test";

import { cn } from "./classes";

describe("semantic typography class merging", () => {
  test("keeps shared type independent of its color and replaces semantic sizing", () => {
    expect(cn("text-ui text-ink", "text-sm text-foreground")).toBe("text-sm text-foreground");
  });

  test("keeps independent font size, color, and line height", () => {
    expect(cn("text-ui leading-ui", "text-ink")).toBe("text-ui leading-ui text-ink");
    expect(cn("text-code leading-code", "text-inverse-text")).toBe(
      "text-code leading-code text-inverse-text",
    );
  });

  test("replaces a semantic font size without discarding color", () => {
    expect(cn("text-ui text-ink", "text-body")).toBe("text-ink text-body");
    expect(cn("text-document text-text-muted", "text-title")).toBe("text-text-muted text-title");
    expect(cn("text-ui leading-ui text-ink", "text-body leading-body")).toBe(
      "text-ink text-body leading-body",
    );
  });
});

describe("semantic control class merging", () => {
  test("fixed code cells override full input width in every pointer mode", () => {
    expect(
      cn(
        "min-h-control-compact w-full rounded-pill px-2",
        "size-control shrink-0 rounded-lg p-0 pointer-coarse:size-control-touch",
      ),
    ).toBe(
      "min-h-control-compact size-control shrink-0 rounded-lg p-0 pointer-coarse:size-control-touch",
    );
    expect(cn("pointer-coarse:min-h-control-touch", "pointer-coarse:min-h-control-small")).toBe(
      "pointer-coarse:min-h-control-small",
    );
  });

  test("semantic spacing and corners replace earlier values", () => {
    expect(
      new Set(cn("w-full rounded-lg px-4", "w-control rounded-pill px-control-inset").split(" ")),
    ).toEqual(new Set(["w-control", "px-control-inset", "rounded-pill"]));
  });
});
