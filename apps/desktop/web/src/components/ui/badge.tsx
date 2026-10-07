import type { ComponentProps } from "react";
import { cn } from "cn";

export function Badge({ className, variant = "default", ...props }: ComponentProps<"span"> & { variant?: "default" | "secondary" | "destructive" | "outline" | "ghost" | "link" }) {
  return <span {...props} data-slot="badge" className={cn("mpai-badge inline-flex items-center rounded-md px-1.5 py-0.5 text-2xs font-medium", variant === "destructive" ? "bg-danger/15 text-danger" : variant === "outline" ? "border border-border" : "bg-control-secondary text-muted", className)} />;
}
