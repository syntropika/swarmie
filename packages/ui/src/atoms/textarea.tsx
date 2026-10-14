import type { ComponentPropsWithRef } from "react";

import { cn } from "../classes";

export type TextareaProps = ComponentPropsWithRef<"textarea"> & {
  appearance?: "surface" | "bare";
};

/** A multiline native field. Consumers supply its persistent label and validation. */
export function Textarea({ className, appearance = "surface", ...props }: TextareaProps) {
  return (
    <textarea
      {...props}
      className={cn(
        "min-h-20 w-full resize-y rounded-control border border-transparent px-3 py-2 text-body leading-body text-ink caret-primary placeholder:text-text-muted focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary disabled:cursor-not-allowed disabled:text-text-muted aria-invalid:border-danger",
        appearance === "bare" ? "bg-transparent focus-visible:outline-none" : "bg-surface-muted",
        className,
      )}
    />
  );
}
