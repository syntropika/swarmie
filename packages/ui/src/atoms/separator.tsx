import type { ComponentPropsWithoutRef } from "react";

import { cn } from "../classes";

export type SeparatorProps = Omit<ComponentPropsWithoutRef<"div">, "children"> & {
  orientation?: "horizontal" | "vertical";
  decorative?: boolean;
};

/** Separate groups with space; opt into a semantic boundary when it separates content. */
export function Separator({
  orientation = "horizontal",
  decorative = true,
  className,
  ...props
}: SeparatorProps) {
  const classes = cn(
    "shrink-0 border-0",
    orientation === "horizontal" ? "h-2 w-full" : "h-full w-2 self-stretch",
    className,
  );

  if (decorative) {
    return <div {...props} aria-hidden="true" className={classes} />;
  }

  return <hr {...props} aria-orientation={orientation} className={classes} />;
}
