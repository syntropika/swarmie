import { cn } from "cn";
import type { ComponentProps, ReactNode } from "react";

import { Input } from "~/src/components/ui/input";

export function InputGroup({
  icon,
  className,
  ...props
}: ComponentProps<typeof Input> & { icon: ReactNode }) {
  return (
    <div className={cn("relative", className)}>
      <span
        className="text-muted pointer-events-none absolute inset-y-0 left-3 flex items-center"
        aria-hidden="true"
      >
        {icon}
      </span>
      <Input {...props} leadingIcon className="w-full" />
    </div>
  );
}
