import * as Primitive from "@radix-ui/react-dropdown-menu";
import { cn } from "cn";
import { Check } from "lucide-react";
import { createContext, useContext, useMemo } from "react";
import type { ReactElement, ReactNode } from "react";

import { floatingClasses, itemClasses } from "./floating";
import { Tooltip } from "./tooltip";

type MenuProps = {
  children: ReactNode;
  "aria-label": string;
  selectionMode?: "single";
  selectedKeys?: Set<string>;
  onAction?: (key: string) => void;
};
const MenuContext = createContext<Pick<MenuProps, "selectionMode" | "selectedKeys" | "onAction">>(
  {},
);
function Root({
  children,
  triggerTooltip,
}: {
  children: [ReactElement, ...ReactNode[]];
  triggerTooltip?: ReactNode;
}) {
  const [button, ...content] = children;
  const trigger = <Primitive.Trigger asChild>{button}</Primitive.Trigger>;
  return (
    <Primitive.Root modal={false}>
      {triggerTooltip === undefined ? (
        trigger
      ) : (
        <Tooltip>
          {trigger}
          {triggerTooltip}
        </Tooltip>
      )}
      {content}
    </Primitive.Root>
  );
}
function Popover({
  children,
  className,
  placement = "bottom start",
}: {
  children: ReactElement<MenuProps>;
  className?: string;
  placement?: "top end" | "bottom end" | "bottom start";
}) {
  const label = children.props["aria-label"];
  const side = placement === "top end" ? "top" : "bottom";
  const align = placement === "bottom start" ? "start" : "end";
  return (
    <Primitive.Portal>
      <Primitive.Content
        side={side}
        align={align}
        sideOffset={8}
        collisionPadding={20}
        aria-labelledby={undefined}
        aria-label={label}
        className={cn(
          floatingClasses,
          "max-h-[var(--radix-dropdown-menu-content-available-height)] min-w-44 p-1.5",
          className,
        )}
      >
        {children}
      </Primitive.Content>
    </Primitive.Portal>
  );
}
function Menu({ children, selectionMode, selectedKeys, onAction }: MenuProps) {
  const context = useMemo(
    () => ({ selectionMode, selectedKeys, onAction }),
    [selectionMode, selectedKeys, onAction],
  );
  return (
    <MenuContext.Provider value={context}>
      {selectionMode === "single" ? (
        <Primitive.RadioGroup value={selectedKeys ? [...selectedKeys][0] : undefined}>
          {children}
        </Primitive.RadioGroup>
      ) : (
        children
      )}
    </MenuContext.Provider>
  );
}
function Item({
  id,
  children,
  isDisabled,
  onAction,
  textValue,
}: {
  id: string;
  children: ReactNode;
  isDisabled?: boolean;
  onAction?: () => void;
  textValue?: string;
}) {
  const menu = useContext(MenuContext);
  const props = {
    disabled: isDisabled,
    textValue,
    className: itemClasses,
    onSelect: () => {
      onAction?.();
      menu.onAction?.(id);
    },
  };
  return menu.selectionMode === "single" ? (
    <Primitive.RadioItem {...props} value={id}>
      {children}
    </Primitive.RadioItem>
  ) : (
    <Primitive.Item {...props}>{children}</Primitive.Item>
  );
}
function ItemIndicator() {
  return (
    <Primitive.ItemIndicator className="ml-auto">
      <Check size={14} />
    </Primitive.ItemIndicator>
  );
}
export const Dropdown = Object.assign(Root, { Popover, Menu, Item, ItemIndicator });
