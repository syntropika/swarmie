import { cva } from "class-variance-authority";
import type { VariantProps } from "class-variance-authority";
import type { ComponentPropsWithoutRef } from "react";

import { cn } from "../classes";
import { Icon } from "./icon";
import type { IconName } from "./icon";

const badgeVariants = cva(
  "inline-flex items-center gap-1.5 rounded-pill px-2.5 py-1 text-xs leading-none font-medium",
  {
    variants: {
      tone: {
        neutral: "bg-surface-muted text-ink",
        agent: "bg-primary-soft text-primary",
        success: "bg-success-soft text-success",
        attention: "bg-attention-soft text-attention",
        danger: "bg-danger-soft text-danger",
      },
    },
    defaultVariants: { tone: "neutral" },
  },
);

const feedbackIcons = {
  success: "check",
  attention: "triangle-attention",
  danger: "circle-close",
} satisfies Record<string, IconName>;

export type BadgeProps = ComponentPropsWithoutRef<"span"> & VariantProps<typeof badgeVariants>;

/** Status tones include a distinct icon; the label must describe the actual state. */
export function Badge({ tone = "neutral", children, className, ...props }: BadgeProps) {
  return (
    <span {...props} className={cn(badgeVariants({ tone }), className)}>
      {tone === "success" || tone === "attention" || tone === "danger" ? (
        <Icon name={feedbackIcons[tone]} className="size-3.5" />
      ) : null}
      {children}
    </span>
  );
}
