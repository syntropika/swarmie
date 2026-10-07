import type { ComponentProps, ReactNode } from "react";
import { Input } from "./input";
import { cn } from "cn";

export function InputGroup({ icon, className, ...props }: ComponentProps<typeof Input> & { icon: ReactNode }) {
  return <div className={cn("relative", className)}><span className="pointer-events-none absolute inset-y-0 left-3 flex items-center text-muted" aria-hidden="true">{icon}</span><Input {...props} className="w-full pl-9" /></div>;
}
