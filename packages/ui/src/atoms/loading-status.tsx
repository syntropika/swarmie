import type { ComponentPropsWithRef } from "react";

import { cn } from "../classes";
import { Icon } from "./icon";

export type LoadingStatusProps = ComponentPropsWithRef<"div">;

/** Announce real pending work with a caller-provided visible label and local placement. */
export function LoadingStatus({ children, className, ...props }: LoadingStatusProps) {
  return (
    <div
      {...props}
      // oxlint-disable-next-line jsx-a11y/prefer-tag-over-role -- This live region announces asynchronous status, not the result of a form calculation.
      role="status"
      aria-atomic="true"
      className={cn("flex items-center gap-2 text-ui text-text-muted", className)}
    >
      <Icon name="loading" className="size-3.5 motion-safe:animate-spin" />
      {children}
    </div>
  );
}
