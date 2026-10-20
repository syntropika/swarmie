import { Spinner } from "@heroui/react";
import type { ComponentPropsWithRef } from "react";

import { cn } from "../classes";

export type LoadingStatusProps = ComponentPropsWithRef<"div">;

/** Pair the vendor spinner with an announced, caller-owned description of pending work. */
export function LoadingStatus({ children, className, ...props }: LoadingStatusProps) {
  return (
    <div
      {...props}
      // oxlint-disable-next-line jsx-a11y/prefer-tag-over-role -- A live region announces pending work rather than a calculation result.
      role="status"
      aria-atomic="true"
      className={cn("flex items-center gap-2 text-sm", className)}
    >
      <Spinner size="sm" aria-hidden="true" />
      {children}
    </div>
  );
}
