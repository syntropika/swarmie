import { cn } from "cn";
import type { ComponentProps } from "react";

import { fieldClasses } from "./input";

export function Textarea({
  className,
  variant = "default",
  ...props
}: ComponentProps<"textarea"> & { variant?: "default" | "editor" }) {
  return (
    <textarea
      {...props}
      data-slot="textarea"
      className={cn(
        "swarmie-textarea min-h-20 py-2",
        fieldClasses,
        variant === "editor" && "file-editor",
        className,
      )}
    />
  );
}
