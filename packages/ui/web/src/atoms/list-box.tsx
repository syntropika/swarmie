import { ListBox as HeroListBox } from "@heroui/react";
import type { ListBox as HeroListBoxTypes, ListBoxProps as HeroListBoxProps } from "@heroui/react";

import { cn } from "../classes";

export type ListBoxProps<T extends object> = HeroListBoxProps<T> & {
  readonly kind?: "default" | "composer";
};

type ItemProps = HeroListBoxTypes["ItemProps"] & {
  readonly kind?: "default" | "composer";
};

function Root<T extends object>({ kind = "default", className, ...props }: ListBoxProps<T>) {
  return (
    <HeroListBox
      {...props}
      data-kind={kind}
      className={cn(kind === "composer" && "p-0", className)}
    />
  );
}

function Item({ kind = "default", className, ...props }: ItemProps) {
  return (
    <HeroListBox.Item
      {...props}
      data-kind={kind}
      className={cn(
        kind === "composer" &&
          "min-h-(--spacing-chrome-menu-row) gap-2 rounded-md text-caption text-foreground data-[hovered=true]:bg-row-hover data-[selected=true]:bg-row-selected [&_[data-slot=description]]:text-caption",
        className,
      )}
    />
  );
}

const ComposerItem = Object.assign(Item, HeroListBox.Item, { Root: Item });

export const ListBox = Object.assign(Root, HeroListBox, { Root, Item: ComposerItem });
