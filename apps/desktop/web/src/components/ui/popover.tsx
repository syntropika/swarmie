import * as Primitive from "@radix-ui/react-popover";
import { cn } from "cn";
import type { ComponentProps } from "react";

import { floatingClasses } from "~/web/src/components/ui/floating";

export const Popover = Primitive.Root;
export const PopoverTrigger = Primitive.Trigger;

export function PopoverContent({
  className,
  sideOffset = 8,
  alignOffset,
  align = "center",
  collisionPadding = 20,
  onOpenAutoFocus,
  ...props
}: ComponentProps<typeof Primitive.Content>) {
  return (
    <Primitive.Portal>
      <Primitive.Content
        {...props}
        align={align}
        sideOffset={sideOffset}
        alignOffset={alignOffset ?? (align === "center" ? 0 : -5)}
        collisionPadding={collisionPadding}
        onOpenAutoFocus={
          onOpenAutoFocus ??
          ((event) => {
            event.preventDefault();
            if (event.currentTarget instanceof HTMLElement) {
              event.currentTarget.focus({ preventScroll: true });
            }
          })
        }
        className={cn(
          floatingClasses,
          "data-[state=open]:animate-popover-enter data-[state=closed]:animate-popover-exit max-h-[var(--radix-popover-content-available-height)] max-w-[var(--radix-popover-content-available-width)] origin-[var(--radix-popover-content-transform-origin)] p-4 motion-reduce:animate-none",
          className,
        )}
      />
    </Primitive.Portal>
  );
}
