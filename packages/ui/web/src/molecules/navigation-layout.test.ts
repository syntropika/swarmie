import { describe, expect, test } from "bun:test";

import {
  NAVIGATION_DEFAULT_WIDTH,
  navigationCanvasOffset,
  navigationLayout,
  readNavigationWidth,
} from "./navigation-layout";
import type { NavigationLayout } from "./navigation-layout";

describe("navigation layout", () => {
  test("closed and temporary navigation leave the reading canvas at the rail edge", () => {
    const pinned: NavigationLayout = { mode: "pinned", width: NAVIGATION_DEFAULT_WIDTH };
    expect(navigationCanvasOffset(pinned)).toBe(340);
    expect(navigationLayout(pinned, { type: "preview" })).toBe(pinned);
    const closed = navigationLayout(pinned, { type: "close" });
    expect(navigationCanvasOffset(closed)).toBe(52);
    const temporary = navigationLayout(closed, { type: "preview" });
    expect(temporary.mode).toBe("temporary");
    expect(navigationCanvasOffset(temporary)).toBe(52);
    expect(navigationCanvasOffset(navigationLayout(temporary, { type: "pin" }))).toBe(340);
  });

  test("resize bounds include the rail and reserve hysteresis before collapsing", () => {
    const layout: NavigationLayout = { mode: "pinned", width: 320 };
    expect(
      navigationLayout(layout, { type: "resize", collapsible: true, totalWidth: 900 }).width,
    ).toBe(468);
    const narrow = navigationLayout(layout, { type: "resize", collapsible: true, totalWidth: 270 });
    expect(narrow).toEqual({ mode: "pinned", width: 238 });
    const closed = navigationLayout(layout, { type: "resize", collapsible: true, totalWidth: 257 });
    expect(closed).toEqual({ mode: "closed", width: 320 });
    expect(
      navigationLayout(layout, { type: "resize", collapsible: true, totalWidth: Number.NaN }),
    ).toBe(layout);
  });

  test("non-collapsible navigation clamps an overdrag and retains the chosen width", () => {
    const layout: NavigationLayout = { mode: "pinned", width: 288 };
    const wide = navigationLayout(layout, { type: "resize", totalWidth: 440, collapsible: false });
    expect(wide).toEqual({ mode: "pinned", width: 388 });
    const minimum = navigationLayout(wide, { type: "resize", totalWidth: 100, collapsible: false });
    expect(minimum).toEqual({ mode: "pinned", width: 238 });
    expect(navigationCanvasOffset(minimum)).toBe(290);
    expect(navigationLayout(minimum, { type: "restore-width", width: wide.width })).toEqual(wide);
  });

  test("dismissal preserves a resized width and never closes pinned navigation", () => {
    const resized = navigationLayout(
      { mode: "temporary", width: 288 },
      { type: "resize", collapsible: true, totalWidth: 440 },
    );
    const dismissed = navigationLayout(resized, { type: "dismiss" });
    expect(dismissed).toEqual({ mode: "closed", width: 388 });
    expect(navigationLayout(dismissed, { type: "preview" })).toEqual({
      mode: "temporary",
      width: 388,
    });
    const pinned = navigationLayout(dismissed, { type: "pin" });
    expect(navigationLayout(pinned, { type: "dismiss" })).toBe(pinned);
  });

  test("stored layout geometry validates missing, malformed, and out-of-range dimensions", () => {
    for (const value of [null, "", " ", "invalid", "Infinity", "-10", "237", "469"]) {
      expect(readNavigationWidth(value)).toBe(288);
    }
    expect(readNavigationWidth("238")).toBe(238);
    expect(readNavigationWidth("468")).toBe(468);
    expect(readNavigationWidth("340.5")).toBe(340.5);
  });
});
