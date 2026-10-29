import type { ComponentProps } from "react";

import { Popover } from "../atoms/popover";
import { cn } from "../classes";

/** Action menus share one compact content inset; informational popovers keep vendor spacing. */
export function ActionMenuDialog({ className, ...props }: ComponentProps<typeof Popover.Dialog>) {
  return (
    <Popover.Dialog
      {...props}
      data-action-menu=""
      className={cn(
        "flex max-w-[calc(100vw-24px)] min-w-0 flex-col p-(--spacing-menu-dialog-inset)",
        className,
      )}
    />
  );
}
