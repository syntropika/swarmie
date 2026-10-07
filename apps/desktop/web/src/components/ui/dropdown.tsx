import * as Primitive from "@radix-ui/react-dropdown-menu";
import { Children, createContext, isValidElement, useContext, type ReactNode } from "react";
import { Check } from "lucide-react";
import { cn } from "cn";
import { floatingClasses, itemClasses } from "./floating";
import { Tooltip } from "./tooltip";

type MenuProps = { children: ReactNode; "aria-label": string; selectionMode?: "single"; selectedKeys?: Set<string>; onAction?: (key: string) => void };
const MenuContext = createContext<Pick<MenuProps, "selectionMode" | "selectedKeys" | "onAction">>({});
function Root({ children, triggerTooltip }: { children: ReactNode; triggerTooltip?: ReactNode }) {
  const parts = Children.toArray(children);
  const trigger = <Primitive.Trigger asChild>{parts[0]}</Primitive.Trigger>;
  return <Primitive.Root modal={false}>{triggerTooltip ? <Tooltip>{trigger}{triggerTooltip}</Tooltip> : trigger}{parts.slice(1)}</Primitive.Root>;
}
function Popover({ children, className, placement = "bottom start" }: { children: ReactNode; className?: string; placement?: "top end" | "bottom end" | "bottom start" }) {
  const menu = Children.toArray(children).find(isValidElement);
  const label = (menu?.props as MenuProps | undefined)?.["aria-label"];
  const [side, align] = placement.split(" ") as ["top" | "bottom", "start" | "end"];
  return <Primitive.Portal><Primitive.Content side={side} align={align} sideOffset={8} collisionPadding={20} aria-labelledby={undefined} aria-label={label}
    className={cn(floatingClasses, "min-w-44 max-h-[var(--radix-dropdown-menu-content-available-height)] p-1.5", className)}>{children}</Primitive.Content></Primitive.Portal>;
}
function Menu({ children, selectionMode, selectedKeys, onAction }: MenuProps) {
  return <MenuContext.Provider value={{ selectionMode, selectedKeys, onAction }}>{selectionMode === "single"
    ? <Primitive.RadioGroup value={selectedKeys ? [...selectedKeys][0] : undefined}>{children}</Primitive.RadioGroup> : children}</MenuContext.Provider>;
}
function Item({ id, children, isDisabled, onAction, textValue }: { id: string; children: ReactNode; isDisabled?: boolean; onAction?: () => void; textValue?: string }) {
  const menu = useContext(MenuContext);
  const props = { disabled: isDisabled, textValue, className: itemClasses, onSelect: () => { onAction?.(); menu.onAction?.(id); } };
  return menu.selectionMode === "single" ? <Primitive.RadioItem {...props} value={id}>{children}</Primitive.RadioItem> : <Primitive.Item {...props}>{children}</Primitive.Item>;
}
function ItemIndicator() { return <Primitive.ItemIndicator className="ml-auto"><Check size={14} /></Primitive.ItemIndicator>; }
export const Dropdown = Object.assign(Root, { Popover, Menu, Item, ItemIndicator });
