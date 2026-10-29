export const NAVIGATION_RAIL_WIDTH = 45;

export const NAVIGATION_DEFAULT_WIDTH = 288;

export const NAVIGATION_MIN_TOTAL_WIDTH = NAVIGATION_RAIL_WIDTH + 238;

export const NAVIGATION_MAX_TOTAL_WIDTH = NAVIGATION_RAIL_WIDTH + 468;

export const NAVIGATION_COLLAPSE_DISTANCE = 32;

export const NAVIGATION_WIDTH_STORAGE_KEY = "swarmie.navigation.width";

export type NavigationMode = "pinned" | "closed" | "temporary";

export type NavigationLayout = {
  mode: NavigationMode;
  width: number;
};

export type NavigationLayoutAction =
  | { type: "preview" }
  | { type: "pin" }
  | { type: "close" }
  | { type: "dismiss" }
  | { type: "resize"; totalWidth: number; collapsible: boolean }
  | { type: "restore-width"; width: number };

/** The reading canvas reserves sidebar width only while navigation is pinned. */
export function navigationCanvasOffset(layout: NavigationLayout): number {
  return NAVIGATION_RAIL_WIDTH + (layout.mode === "pinned" ? layout.width : 0);
}

/** External storage never admits invalid or out-of-bounds layout dimensions. */
export function readNavigationWidth(value: string | null): number {
  if (value === null || value.trim() === "") {
    return NAVIGATION_DEFAULT_WIDTH;
  }
  const width = Number(value);
  if (
    !Number.isFinite(width) ||
    width < NAVIGATION_MIN_TOTAL_WIDTH - NAVIGATION_RAIL_WIDTH ||
    width > NAVIGATION_MAX_TOTAL_WIDTH - NAVIGATION_RAIL_WIDTH
  ) {
    return NAVIGATION_DEFAULT_WIDTH;
  }
  return width;
}

/** Closing retains the chosen width; a temporary sidebar never changes canvas geometry. */
export function navigationLayout(
  layout: NavigationLayout,
  action: NavigationLayoutAction,
): NavigationLayout {
  switch (action.type) {
    case "preview": {
      return layout.mode === "closed" ? { ...layout, mode: "temporary" } : layout;
    }
    case "pin": {
      return { ...layout, mode: "pinned" };
    }
    case "close": {
      return { ...layout, mode: "closed" };
    }
    case "dismiss": {
      return layout.mode === "temporary" ? { ...layout, mode: "closed" } : layout;
    }
    case "restore-width": {
      return { ...layout, width: readNavigationWidth(String(action.width)) };
    }
    case "resize": {
      if (!Number.isFinite(action.totalWidth)) {
        return layout;
      }
      if (
        action.collapsible &&
        action.totalWidth < NAVIGATION_MIN_TOTAL_WIDTH - NAVIGATION_COLLAPSE_DISTANCE
      ) {
        return { ...layout, mode: "closed" };
      }
      const totalWidth = Math.min(
        NAVIGATION_MAX_TOTAL_WIDTH,
        Math.max(NAVIGATION_MIN_TOTAL_WIDTH, action.totalWidth),
      );
      return { ...layout, width: totalWidth - NAVIGATION_RAIL_WIDTH };
    }
    default: {
      throw new Error("Unexpected navigation layout action.");
    }
  }
}
