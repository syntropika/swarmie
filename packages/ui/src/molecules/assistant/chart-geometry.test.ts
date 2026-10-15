import { describe, expect, test } from "bun:test";

import { chartGeometry } from "./chart-geometry";

describe("signed chart geometry", () => {
  test("negative bars extend below zero without clamping the source value", () => {
    const graph = chartGeometry(["Loss", "Gain"], [{ values: [-10, 20] }], false, true);
    expect(graph.minimum).toBe(-10);
    expect(graph.maximum).toBe(20);
    expect(graph.y(-10)).toBeGreaterThan(graph.y(0));
    expect(graph.y(20)).toBeLessThan(graph.y(0));
    expect(graph.x(0)).toBe(170);
    expect(graph.x(1)).toBe(430);
  });
  test("diverging stacks accumulate each sign independently", () => {
    const graph = chartGeometry(
      ["A"],
      [{ values: [-10] }, { values: [20] }, { values: [-5] }, { values: [3] }],
      true,
      true,
    );
    expect(graph.baseline(2, 0)).toBe(-10);
    expect(graph.baseline(3, 0)).toBe(20);
    expect(graph.minimum).toBe(-15);
    expect(graph.maximum).toBe(23);
  });
  test("all zero and empty data keep finite coordinates", () => {
    const graph = chartGeometry([], [], false, false);
    expect(Number.isFinite(graph.y(0))).toBe(true);
    expect(Number.isFinite(graph.x(0))).toBe(true);
  });
});
