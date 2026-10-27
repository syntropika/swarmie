import { Popover as HeroPopover } from "@heroui/react";
import type {
  Popover as HeroPopoverTypes,
  PopoverContentProps as HeroPopoverContentProps,
} from "@heroui/react";
import type { ComponentProps } from "react";

import { cn } from "../classes";

export type PopoverContentProps = HeroPopoverContentProps & {
  /** Compact application controls share the composer's menu surface and accent tokens. */
  readonly kind?: "default" | "composer";
};

function Content({ kind = "default", className, ...props }: PopoverContentProps) {
  const geometry =
    kind === "composer"
      ? "w-[260px] max-w-[calc(100vw-24px)] rounded-md border border-composer-border bg-overlay text-foreground [--accent:var(--color-brand-accent)] [--accent-foreground:var(--color-brand-accent-foreground)] [&_.popover__dialog]:p-3"
      : undefined;
  return (
    <HeroPopover.Content
      {...props}
      data-kind={kind}
      className={
        typeof className === "function"
          ? (state) => cn(geometry, className(state))
          : cn(geometry, className)
      }
    />
  );
}

function Root(props: ComponentProps<typeof HeroPopover>) {
  return <HeroPopover {...props} />;
}

// oxlint-disable-next-line no-redeclare -- Preserve the compound vendor value and prop namespace.
export const Popover = Object.assign(Root, HeroPopover, { Content });
export type Popover = Omit<HeroPopoverTypes, "ContentProps"> & {
  ContentProps: PopoverContentProps;
};
export type { PopoverProps } from "@heroui/react";
