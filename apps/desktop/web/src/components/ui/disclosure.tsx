import * as Primitive from "@radix-ui/react-collapsible";
import { ChevronDown } from "lucide-react";
import { Children, isValidElement, type ReactNode } from "react";

function Summary({ children }: { children: ReactNode }) { return <>{children}</>; }
function Root({ children, className = "", defaultOpen = false, onOpenChange }: {
  children: ReactNode; className?: string; defaultOpen?: boolean; onOpenChange?: (open: boolean) => void;
}) {
  const content = Children.toArray(children);
  const heading = content.find((child) => isValidElement(child) && child.type === Summary);
  return <Primitive.Root defaultOpen={defaultOpen} onOpenChange={onOpenChange} className={`workspace-disclosure ${className}`}>
    <Primitive.Trigger className="group flex w-full items-center justify-between gap-2 rounded-lg px-2 py-2 text-left text-sm font-medium outline-none hover:bg-control-hover focus-visible:ring-2 focus-visible:ring-focus">
      {heading}<ChevronDown size={14} className="transition-transform group-data-[state=open]:rotate-180 motion-reduce:transition-none" />
    </Primitive.Trigger>
    <Primitive.Content className="pt-2">{content.filter((child) => child !== heading)}</Primitive.Content>
  </Primitive.Root>;
}
export const Disclosure = Object.assign(Root, { Summary });
