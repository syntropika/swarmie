import { Separator as HeroSeparator } from "@heroui/react";

import { cn } from "../classes";

export { Separator } from "@heroui/react";
export type { SeparatorProps } from "@heroui/react";

/** A decorative boundary between compact header controls. */
export function ChromeSeparator({ className }: { readonly className?: string }) {
  return (
    <span
      aria-hidden="true"
      data-chrome-separator=""
      className={cn(
        "pointer-events-none flex h-(--spacing-chrome-divider) w-px shrink-0 self-center",
        className,
      )}
    >
      <HeroSeparator orientation="vertical" />
    </span>
  );
}
