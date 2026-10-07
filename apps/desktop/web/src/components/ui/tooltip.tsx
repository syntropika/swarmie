import * as Primitive from "@radix-ui/react-tooltip";
import { Children, type ReactNode } from "react";
import { cn } from "cn";
import { floatingClasses } from "./floating";

export function Tooltip({ children, delay = 500 }: { children: ReactNode; delay?: number }) {
  const parts = Children.toArray(children);
  return <Primitive.Provider delayDuration={delay}><Primitive.Root><Primitive.Trigger asChild>{parts[0]}</Primitive.Trigger>{parts.slice(1)}</Primitive.Root></Primitive.Provider>;
}
export function TooltipContent({ children, placement = "top", offset = 6 }: { children: ReactNode; placement?: "top" | "bottom" | "left" | "right"; offset?: number }) {
  return <Primitive.Portal><Primitive.Content side={placement} sideOffset={offset} className={cn(floatingClasses, "px-3 py-1.5 text-sm")}>{children}</Primitive.Content></Primitive.Portal>;
}
