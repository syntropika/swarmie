import type { ComponentProps } from "react";

import { Button } from "../atoms/button";
import type { ButtonProps } from "../atoms/button";
import { cn } from "../classes";

/** A settings row shares full-width feedback while preserving independent controls. */
function Root({ className, ...props }: ComponentProps<"div">) {
  return (
    <div
      {...props}
      data-slot="action-row"
      className={cn("flex min-w-0 items-center gap-3 px-4 py-4", className)}
    />
  );
}

function Trigger({ className, ...props }: Omit<ButtonProps, "variant">) {
  const layout = "h-auto min-w-0 flex-1 justify-start gap-3 px-0 py-0 whitespace-normal text-left";
  return (
    <Button
      {...props}
      data-slot="action-row-trigger"
      variant="ghost"
      className={
        typeof className === "function"
          ? (state) => cn(layout, className(state))
          : cn(layout, className)
      }
    />
  );
}

export const ActionRow = Object.assign(Root, { Root, Trigger });
