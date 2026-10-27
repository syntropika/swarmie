import { Dropdown as HeroDropdown } from "@heroui/react";
import type { ComponentProps } from "react";

import { cn } from "../classes";

export { Dropdown } from "@heroui/react";
export type { DropdownProps } from "@heroui/react";

/** Action menus keep compact application typography while the vendor owns keyboard and dismissal. */
export function ActionMenuPopover({
  className,
  kind = "application",
  ...props
}: Omit<ComponentProps<typeof HeroDropdown.Popover>, "className"> & {
  className?: string;
  kind?: "application" | "composer" | "composerPanel";
}) {
  let width = "w-[210px]";
  if (kind === "composer") {
    width = "w-[260px]";
  }
  if (kind === "composerPanel") {
    width =
      "w-(--trigger-width) rounded-composer p-1.5 [&_[role=menu]]:max-h-72 [&_[role=menu]]:overflow-y-auto [&_[data-slot=header]]:px-2 [&_[data-slot=header]]:py-1 [&_[data-slot=header]]:text-chrome [&_[data-slot=header]]:font-normal [&_[data-slot=header]]:text-muted";
  }
  return (
    <HeroDropdown.Popover
      {...props}
      className={cn(
        "max-w-[calc(100vw-24px)] rounded-md border border-composer-border bg-overlay p-1 text-foreground [&_.label]:text-chrome [&_.label]:font-normal [&_[role=menu]]:p-0 [&_[role=menuitem]]:min-h-(--spacing-chrome-menu-row) [&_[role=menuitem]]:gap-2 [&_[role=menuitem]]:rounded-small [&_[role=menuitem]]:px-2.5 [&_[role=menuitem]]:py-2 [&_[role=menuitem]]:text-chrome [&_[role=menuitem]]:font-normal [&_[role=menuitem]]:data-[focused=true]:bg-default-hover [&_[role=menuitem]]:data-[hovered=true]:bg-default-hover [&_svg]:size-3.5",
        width,
        className,
      )}
    />
  );
}
