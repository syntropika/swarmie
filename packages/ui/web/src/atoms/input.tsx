import type { ComponentPropsWithRef } from "react";

import { cn } from "../classes";

export type InputProps = ComponentPropsWithRef<"input"> & {
  adornment?: "none" | "trailing";
  controlSize?: "default" | "large" | "compact";
  focusFeedback?: "outline" | "ring";
};

const inputSizes = {
  default: "min-h-control px-control-inset py-1 pointer-coarse:min-h-control-touch",
  compact: "min-h-control-compact px-2 py-0 pointer-coarse:min-h-control-touch",
  large: "min-h-control-entry px-4 py-2",
};

/** A native input; provide a persistent label directly or through Field. */
export function Input({
  className,
  type = "text",
  adornment = "none",
  controlSize = "default",
  focusFeedback = "outline",
  ...props
}: InputProps) {
  return (
    <input
      {...props}
      type={type}
      className={cn(
        "w-full border border-transparent bg-surface-muted text-body leading-body text-ink caret-primary transition-colors duration-150 ease-swarmie placeholder:text-text-muted disabled:cursor-not-allowed disabled:bg-surface-muted disabled:text-text-muted aria-invalid:border-danger aria-invalid:focus-visible:outline-danger motion-reduce:transition-none",
        focusFeedback === "ring"
          ? "focus:assistant-field-focus outline-none aria-invalid:focus-visible:outline-2"
          : "focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary",
        inputSizes[controlSize],
        "rounded-pill",
        adornment === "trailing" && (controlSize === "large" ? "pr-12" : "pr-10"),
        className,
      )}
    />
  );
}
