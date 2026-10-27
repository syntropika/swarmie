import { useSource } from "@swarmie/client-ui";
import { ChromeButton, Popover, copyText as writeClipboard } from "@swarmie/ui-web";
import type { ComponentType, ReactNode } from "react";
import { useState } from "react";

import { useClient } from "./client/context";

export type Icon = ComponentType<{ className?: string; size?: number; strokeWidth?: number }>;
export const iconStyle = "size-3.5 shrink-0 stroke-(length:--stroke-chrome-icon)";
export function IconButton({
  icon: Icon,
  label,
  onPress,
  active = false,
  size = "application",
  disabled = false,
}: {
  icon: Icon;
  label: string;
  onPress?: () => void;
  active?: boolean;
  size?: "application" | "rail" | "composer" | "tabClose";
  disabled?: boolean;
}) {
  return (
    <ChromeButton
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
  size?: "application" | "composer" | "composerCap" | "action";
  placement?: "bottom end" | "bottom start" | "top start" | "top end";
  children: (close: () => void) => ReactNode;
}) {
  const [open, setOpen] = useState(false);
  return (
    <Popover isOpen={open} onOpenChange={setOpen}>
      <ChromeButton size={size} aria-label={label} title={label} selected={open}>
        {Icon ? <Icon className={iconStyle} /> : null}
        {text}
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
}: {
  icon?: Icon | undefined;
  children: ReactNode;
  onClick: () => void;
  detail?: string;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="flex min-h-8 w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-xs text-foreground hover:bg-row-hover focus-visible:bg-row-hover"
    >
      {Icon ? <Icon className={iconStyle} /> : null}
      <span className="min-w-0 flex-1">{children}</span>
      {detail !== undefined && detail !== "" ? <span className="text-muted">{detail}</span> : null}
    </button>
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
