import type { ComponentProps } from "react";
import { cn } from "cn";

export const fieldClasses = "rounded-lg border border-border bg-control-secondary px-3 text-sm text-foreground outline-none placeholder:text-muted focus-visible:ring-2 focus-visible:ring-focus disabled:opacity-40 disabled:cursor-not-allowed";
export function Input({ className, ...props }: ComponentProps<"input">) {
  return <input {...props} data-slot="input" className={cn("mpai-input h-9 min-w-0", fieldClasses, className)} />;
}
