import * as Primitive from "@radix-ui/react-collapsible";
import { ChevronDown } from "lucide-react";
import type { ReactElement, ReactNode } from "react";

function Summary({ children }: { children: ReactNode }) {
  return <span className="flex min-w-0 flex-1 items-center gap-2">{children}</span>;
}
function Root({
  children,
  className = "",
  variant = "default",
  defaultOpen = false,
  onOpenChange,
}: {
  children: [ReactElement, ...ReactNode[]];
  className?: string;
  variant?: "default" | "activity";
  defaultOpen?: boolean;
  onOpenChange?: (open: boolean) => void;
}) {
  const [heading, ...content] = children;
  return (
    <Primitive.Root
      defaultOpen={defaultOpen}
      onOpenChange={onOpenChange}
      className={variant === "activity" ? "tool-activity" : className}
    >
      <Primitive.Trigger className="group hover:bg-control-hover focus-visible:ring-focus flex w-full items-center justify-between gap-2 rounded-lg px-2 py-2 text-left text-sm font-medium outline-none focus-visible:ring-2">
        {heading}
        <ChevronDown
          size={14}
          className="transition-transform group-data-[state=open]:rotate-180 motion-reduce:transition-none"
        />
      </Primitive.Trigger>
      <Primitive.Content className="pt-2">{content}</Primitive.Content>
    </Primitive.Root>
  );
}
export const Disclosure = Object.assign(Root, { Summary });
