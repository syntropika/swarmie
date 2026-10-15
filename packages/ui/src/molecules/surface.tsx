import type { ComponentPropsWithRef } from "react";

import { cn } from "../classes";

export type SurfaceProps = ComponentPropsWithRef<"div"> & {
  elevation?: "flat" | "raised";
  inset?: "none" | "compact" | "comfortable" | "spacious";
};

/** A neutral working surface; consumers own headings, landmarks, and application behavior. */
export function Surface({
  elevation = "flat",
  inset = "comfortable",
  className,
  ...props
}: SurfaceProps) {
  return (
    <div
      {...props}
      className={cn(
        "min-w-0 rounded-panel bg-surface text-ink",
        elevation === "raised" ? "shadow-raised" : "border border-transparent",
        inset === "compact" && "p-3",
        inset === "comfortable" && "p-panel-inset",
        inset === "spacious" && "p-6",
        className,
      )}
    />
  );
}

/** Rows own settings insets while their Surface container remains unpadded. */
export function SurfaceRow({ className, ...props }: ComponentPropsWithRef<"div">) {
  return (
    <div
      {...props}
      className={cn(
        "flex min-w-0 flex-wrap items-center justify-between gap-3 px-row-inset py-2",
        className,
      )}
    />
  );
}
