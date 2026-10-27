import { clsx } from "clsx";
import type { ClassValue } from "clsx";
import { extendTailwindMerge } from "tailwind-merge";

// Register semantic tokens so overrides participate in the same groups as native utilities.
const mergeClasses = extendTailwindMerge({
  extend: {
    theme: {
      spacing: [
        "control",
        "control-compact",
        "control-small",
        "control-large",
        "control-entry",
        "control-touch",
        "control-inset",
        "row-inset",
        "panel-inset",
        "safe",
      ],
      radius: [
        "small",
        "composer",
        "menu",
        "control",
        "panel",
        "illustration",
        "marketing-card",
        "marketing-shell",
        "pill",
      ],
    },
    classGroups: {
      leading: [{ leading: ["ui", "body", "content", "code", "notice", "composer"] }],
      "font-size": [
        {
          text: [
            "notice",
            "brand",
            "brand-large",
            "wordmark",
            "marketing-display",
            "marketing-section",
            "display",
            "headline",
            "title",
            "body",
            "ui",
            "label",
            "content",
            "document",
            "caption",
            "chrome",
            "navigation",
            "navigation-heading",
            "navigation-label",
            "panel-switch",
            "conversation",
            "code",
          ],
        },
      ],
    },
  },
});

/** Merge utility overrides without requiring consumers to know the base classes. */
export function cn(...values: ClassValue[]): string {
  return mergeClasses(clsx(values));
}
