import { expect, test } from "bun:test";

import { positionRailWindowIndexes } from "./conversation-map-geometry";

test("a large message rail stays bounded while retaining keyboard and reader positions", () => {
  const count = 100_000;
  const indexes = new Map([
    ["reader", 0],
    ["keyboard", count - 1],
  ]);
  const window = positionRailWindowIndexes(count, indexes, 12_000, 120, "reader", "keyboard");
  expect(window.length).toBeLessThan(30);
  expect(window).toContain(0);
  expect(window).toContain(count - 1);
  expect(window).toContain(1000);
  expect(new Set(window).size).toBe(window.length);
  expect(window.every((index) => index >= 0 && index < count)).toBe(true);
});

test("an empty rail has no positions and missing retained messages are ignored", () => {
  expect(positionRailWindowIndexes(0, new Map(), 0, 120, "retired")).toEqual([]);
  expect(positionRailWindowIndexes(3, new Map(), 0, 120, "retired")).toEqual([0, 1, 2]);
});
