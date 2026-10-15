import * as Primitive from "@radix-ui/react-tooltip";
import { cn } from "cn";
import type { ReactElement, ReactNode } from "react";

import { floatingClasses } from "~/web/src/components/ui/floating";

export function Tooltip({
  children,
  delay = 500,
}: {
  children: [ReactElement, ...ReactNode[]];
  delay?: number;
}) {
  const [trigger, ...content] = children;
  return (
    <Primitive.Provider delayDuration={delay}>
      <Primitive.Root>
        <Primitive.Trigger asChild>{trigger}</Primitive.Trigger>
        {content}
      </Primitive.Root>
    </Primitive.Provider>
  );
}
export function TooltipContent({
  children,
  placement = "top",
  offset = 6,
}: {
  children: ReactNode;
  placement?: "top" | "bottom" | "left" | "right";
  offset?: number;
}) {
  return (
    <Primitive.Portal>
      <Primitive.Content
        side={placement}
        sideOffset={offset}
        className={cn(floatingClasses, "px-3 py-1.5 text-sm")}
      >
        {children}
      </Primitive.Content>
    </Primitive.Portal>
  );
}
