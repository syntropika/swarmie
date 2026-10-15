import { cva } from "class-variance-authority";
import { cn } from "cn";
import type { ComponentProps } from "react";

export const fieldClasses =
  "rounded-lg border border-border bg-control-secondary px-3 text-sm text-foreground outline-none placeholder:text-muted focus-visible:ring-2 focus-visible:ring-focus disabled:opacity-40 disabled:cursor-not-allowed";
const inputVariants = cva("swarmie-input h-9 min-w-0", {
  variants: { variant: { default: "", search: "sidebar-search" } },
  defaultVariants: { variant: "default" },
});
export function Input({
  className,
  leadingIcon = false,
  variant = "default",
  ...props
}: ComponentProps<"input"> & { leadingIcon?: boolean; variant?: "default" | "search" }) {
  return (
    <input
      {...props}
      data-slot="input"
      className={cn(inputVariants({ variant }), fieldClasses, leadingIcon && "pl-9", className)}
    />
  );
}
