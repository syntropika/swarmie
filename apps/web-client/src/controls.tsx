import { useSource } from "@swarmie/client-ui";
import {
  ActionMenuDialog,
  Button,
  ChromeButton,
  Description,
  ListBox,
  Popover,
  copyText as writeClipboard,
} from "@swarmie/ui-web";
import type { ChromeButtonProps } from "@swarmie/ui-web";
import { ChevronDown } from "lucide-react";
import type { ComponentType, ReactNode } from "react";
import { createContext, useCallback, useContext, useState } from "react";

import { useClient } from "./client/context";

const MenuCloseContext = createContext<(() => void) | null>(null);

export type Icon = ComponentType<{ className?: string; size?: number; strokeWidth?: number }>;
export const iconStyle = "size-(--spacing-chrome-icon) shrink-0";
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
  detail,
  size = "application",
  placement = "bottom end",
  showChevron = false,
  renderTrigger,
  children,
}: {
  label: string;
  icon?: Icon | undefined;
  text?: string;
  detail?: string;
  size?: "application" | "rail" | "composer" | "composerAdd" | "composerCap" | "action";
  placement?: "bottom end" | "bottom start" | "top start" | "top end";
  showChevron?: boolean;
  renderTrigger?: (state: { open: boolean; label: string }) => ReactNode;
  children: (close: () => void) => ReactNode;
}) {
  const [open, setOpen] = useState(false);
  const close = useCallback(() => {
    setOpen(false);
  }, []);
  const value = [text, detail].filter(Boolean).join(" · ");
  const name = value === "" ? label : `${value}: ${label}`;
  return (
    <Popover isOpen={open} onOpenChange={setOpen}>
      {renderTrigger ? (
        renderTrigger({ open, label: name })
      ) : (
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
          {detail !== undefined && detail !== "" ? (
            <span className="shrink-0 text-muted">{detail}</span>
          ) : null}
          {showChevron ? <ChevronDown className={iconStyle} /> : null}
        </ChromeButton>
      )}
      <Popover.Content placement={placement} className="z-50">
        <ActionMenuDialog aria-label={label}>
          <MenuCloseContext value={close}>{children(close)}</MenuCloseContext>
        </ActionMenuDialog>
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
  disabled = false,
}: {
  icon?: Icon | undefined;
  children: ReactNode;
  onClick: () => void;
  detail?: string;
  expanded?: boolean;
  disabled?: boolean;
}) {
  const close = useContext(MenuCloseContext);
  return (
    <Button
      type="button"
      variant="ghost"
      {...(expanded === undefined ? {} : { "aria-expanded": expanded })}
      isDisabled={disabled}
      onPress={() => {
        if (expanded === undefined) {
          close?.();
        }
        onClick();
      }}
      className="w-full justify-start text-left"
    >
      {Icon ? <Icon /> : null}
      <span className="min-w-0 flex-1">{children}</span>
      {detail !== undefined && detail !== "" ? <span className="text-muted">{detail}</span> : null}
    </Button>
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
  const close = useContext(MenuCloseContext);
  return (
    <ListBox
      aria-label={label}
      selectionMode="single"
      selectionBehavior="toggle"
      disallowEmptySelection
      selectedKeys={value === undefined || value === null ? [] : [value]}
    >
      {items.map(({ id, label: itemLabel, description, icon: Icon }) => (
        <ListBox.Item
          key={id}
          id={id}
          textValue={itemLabel}
          aria-label={itemLabel}
          onPress={() => {
            close?.();
            onAction(id);
          }}
        >
          {Icon ? <Icon /> : null}
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
  return (
    <p data-slot="menu-caption" className="px-(--spacing-menu-row-x) py-0.5 text-menu text-muted">
      {children}
    </p>
  );
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
