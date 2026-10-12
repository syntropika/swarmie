import type { ComponentProps } from "react";
import { cn } from "cn";
import { fieldClasses } from "./input";

export function Textarea({ className, ...props }: ComponentProps<"textarea">) {
  return <textarea {...props} data-slot="textarea" className={cn("swarmie-textarea min-h-20 py-2", fieldClasses, className)} />;
}
