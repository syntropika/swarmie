import { cn } from "cn";
import type { ComponentProps } from "react";

export function Badge({
  className,
  variant = "default",
  ...props
}: ComponentProps<"span"> & {
  variant?: "default" | "secondary" | "destructive" | "outline" | "ghost" | "link";
}) {
  return (
    <span
      {...props}
      data-slot="badge"
      className={cn(
        "inline-flex items-center rounded-md px-1.5 py-0.5 text-xs font-medium",
        {
          destructive: "bg-danger/15 text-danger",
          outline: "border border-border",
          default: "bg-control-secondary text-muted",
          secondary: "bg-control-secondary text-muted",
          ghost: "bg-control-secondary text-muted",
          link: "bg-control-secondary text-muted",
        }[variant],
        className,
      )}
    />
  );
}
