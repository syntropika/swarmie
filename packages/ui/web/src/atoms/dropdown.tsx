import { Dropdown as HeroDropdown } from "@heroui/react";
import type { ComponentProps } from "react";

import { cn } from "../classes";

export { Dropdown } from "@heroui/react";
export type { DropdownProps } from "@heroui/react";

/** Application menus add width and scrolling layout to the stock vendor surface. */
export function ActionMenuPopover({
  className,
  kind = "application",
  ...props
}: Omit<ComponentProps<typeof HeroDropdown.Popover>, "className"> & {
  className?: string;
  kind?: "application" | "composer" | "composerPanel";
}) {
  let width = "w-max min-w-0 md:min-w-0";
  if (kind === "composer") {
    width = "w-(--spacing-menu-wide)";
  }
  if (kind === "composerPanel") {
    width = "w-(--trigger-width) [&_[role=menu]]:max-h-72 [&_[role=menu]]:overflow-y-auto";
  }
  return (
    <HeroDropdown.Popover
      {...props}
      data-action-menu=""
      className={cn(
        "max-w-[calc(100vw-24px)] overflow-y-auto overscroll-contain",
        width,
        className,
      )}
    />
  );
}
