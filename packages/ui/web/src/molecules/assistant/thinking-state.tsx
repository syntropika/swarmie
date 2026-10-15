import type { ComponentPropsWithRef } from "react";

import { cn } from "../../classes";

/** The selected brightness sweep may include a caller-owned elapsed label and activity dot. */
export function ThinkingState({
  className,
  children,
  elapsed,
  activity = false,
  ...props
}: ComponentPropsWithRef<"span"> & { elapsed?: string; activity?: boolean }) {
  return (
    <span
      {...props}
      className={cn(
        "inline-flex items-center gap-2 text-ui leading-ui font-medium text-text-muted",
        className,
      )}
    >
      {activity ? (
        <span
          aria-hidden="true"
          className="size-1.5 rounded-full bg-primary motion-safe:animate-pulse"
        />
      ) : null}
      <span
        key={typeof children === "string" ? children : undefined}
        className="motion-safe:animate-assistant-receipt"
      >
        <span className="motion-safe:assistant-shimmer">{children}</span>
      </span>
      {elapsed === undefined ? null : <span className="text-xs tabular-nums">{elapsed}</span>}
    </span>
  );
}
