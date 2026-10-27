import { useSource } from "@swarmie/client-ui";
import {
  ChromeButton,
  Description,
  ListBox,
  Popover,
  copyText as writeClipboard,
} from "@swarmie/ui-web";
import type { ChromeButtonProps } from "@swarmie/ui-web";
import type { ComponentType, ReactNode } from "react";
import { useState } from "react";

import { useClient } from "./client/context";

export type Icon = ComponentType<{ className?: string; size?: number; strokeWidth?: number }>;
export const iconStyle =
  "size-(--spacing-chrome-icon) shrink-0 stroke-(length:--stroke-chrome-icon)";
export function IconButton({
  icon: Icon,
  label,
  onPress,
  active = false,
  size = "application",
  disabled = false,
  ...semantics
}: {
  icon: Icon;
  label: string;
  onPress?: () => void;
  active?: boolean;
  size?: "application" | "rail" | "composer" | "tabClose";
  disabled?: boolean;
} & Pick<ChromeButtonProps, "aria-current" | "aria-pressed" | "aria-expanded" | "aria-controls">) {
  return (
    <ChromeButton
      {...semantics}
      size={size}
      aria-label={label}
      title={label}
      selected={active}
      isDisabled={disabled}
      {...(onPress === undefined ? {} : { onPress })}
    >
      <Icon className={iconStyle} />
    </ChromeButton>
  );
}

export function Menu({
  label,
  icon: Icon,
  text,
  size = "application",
  placement = "bottom end",
  children,
}: {
  label: string;
  icon?: Icon | undefined;
  text?: string;
  size?: "application" | "rail" | "composer" | "composerCap" | "action";
  placement?: "bottom end" | "bottom start" | "top start" | "top end";
  children: (close: () => void) => ReactNode;
}) {
  const [open, setOpen] = useState(false);
  const name = text !== undefined && text !== "" ? `${text}: ${label}` : label;
  return (
    <Popover isOpen={open} onOpenChange={setOpen}>
      <ChromeButton
        size={size}
        aria-label={name}
        title={name}
        selected={open}
        className={size === "composerCap" ? "max-w-full" : ""}
      >
        {Icon ? <Icon className={iconStyle} /> : null}
        {text === undefined ? null : (
          <span
            className={
              size === "composerCap" ? "max-w-(--spacing-composer-label) min-w-0 truncate" : ""
            }
          >
            {text}
          </span>
        )}
      </ChromeButton>
      <Popover.Content placement={placement} kind="composer" className="z-50">
        <Popover.Dialog aria-label={label}>
          {children(() => {
            setOpen(false);
          })}
        </Popover.Dialog>
      </Popover.Content>
    </Popover>
  );
}

export function Item({
  icon: Icon,
  children,
  onClick,
  detail,
  expanded,
}: {
  icon?: Icon | undefined;
  children: ReactNode;
  onClick: () => void;
  detail?: string;
  expanded?: boolean;
}) {
  return (
    <button
      type="button"
      aria-expanded={expanded}
      onClick={onClick}
      className="flex min-h-(--spacing-chrome-menu-row) w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-caption text-foreground hover:bg-row-hover focus-visible:bg-row-hover"
    >
      {Icon ? <Icon className={iconStyle} /> : null}
      <span className="min-w-0 flex-1">{children}</span>
      {detail !== undefined && detail !== "" ? <span className="text-muted">{detail}</span> : null}
    </button>
  );
}

export function ChoiceList<T extends string>({
  label,
  value,
  items,
  onAction,
}: {
  label: string;
  value: T | null | undefined;
  items: readonly { id: T; label: string; description?: string | undefined; icon?: Icon }[];
  onAction: (id: T) => void;
}) {
  return (
    <ListBox
      aria-label={label}
      selectionMode="single"
      selectionBehavior="toggle"
      disallowEmptySelection
      selectedKeys={value === undefined || value === null ? [] : [value]}
      kind="composer"
    >
      {items.map(({ id, label: itemLabel, description, icon: Icon }) => (
        <ListBox.Item
          key={id}
          id={id}
          textValue={itemLabel}
          aria-label={itemLabel}
          kind="composer"
          onPress={() => {
            onAction(id);
          }}
        >
          {Icon ? <Icon className={iconStyle} /> : null}
          <span className="min-w-0 flex-1">
            <span className="block">{itemLabel}</span>
            {description === undefined ? null : <Description>{description}</Description>}
          </span>
          <ListBox.ItemIndicator />
        </ListBox.Item>
      ))}
    </ListBox>
  );
}

export function Caption({ children }: { children: ReactNode }) {
  return <p className="px-2 pt-2 pb-1 text-chrome text-muted">{children}</p>;
}

export function useChat() {
  const { activeConversation, roster, shell } = useClient();

  const layout = useSource(shell);
  useSource(roster);
  return roster.getSnapshot().find((chat) => chat.id === layout.active) ?? activeConversation();
}

function clipboardFailed(error: unknown) {
  console.error(error);
}
export function copyText(text: string): void {
  writeClipboard(text).catch(clipboardFailed);
}
