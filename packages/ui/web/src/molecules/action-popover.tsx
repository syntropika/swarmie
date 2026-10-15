import { createContext, useCallback, useContext, useId, useMemo, useRef, useState } from "react";
import type { ComponentPropsWithRef, ReactNode, Ref } from "react";

import { Button } from "../atoms/button";
import { Icon } from "../atoms/icon";
import { cn } from "../classes";

export type ActionPopoverRootProps = {
  children: ReactNode;
  onOpenChange?: (open: boolean) => void;
};

export type ActionPopoverTriggerProps = ComponentPropsWithRef<"button"> & {
  size?: "icon" | "navigation" | "menu";
  variant?: "ghost" | "quiet" | "disclosure";
};

export type ActionPopoverContentProps = ComponentPropsWithRef<"dialog">;

export type ActionPopoverItemProps = ComponentPropsWithRef<"button">;

type ActionPopoverContextValue = {
  id: string;
  open: boolean;
  close: () => void;
  show: (options?: { focus?: boolean }) => void;
  registerContent: (content: HTMLDialogElement | null) => void;
  registerTrigger: (trigger: HTMLButtonElement | null) => void;
  handleToggle: (content: HTMLDialogElement, open: boolean) => void;
};

const ActionPopoverContext = createContext<ActionPopoverContextValue | undefined>(undefined);

function useActionPopoverContext() {
  const context = useContext(ActionPopoverContext);
  if (!context) {
    throw new Error("Action popover parts must be rendered inside ActionPopoverRoot.");
  }
  return context;
}

function assignRef<T>(ref: Ref<T> | undefined, element: T | null) {
  if (typeof ref === "function") {
    return ref(element);
  }
  if (ref) {
    ref.current = element;
  }
}

/** Native popovers supply Escape, light dismissal, and focus order without menu widget roles. */
export function ActionPopoverRoot({ children, onOpenChange }: ActionPopoverRootProps) {
  const id = useId();
  const [open, setOpen] = useState(false);
  const contentRef = useRef<HTMLDialogElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const focusOnOpen = useRef(true);
  const close = useCallback(() => {
    contentRef.current?.hidePopover();
  }, []);
  const show = useCallback(({ focus = true }: { focus?: boolean } = {}) => {
    const content = contentRef.current;
    if (content && !content.matches(":popover-open")) {
      focusOnOpen.current = focus;
      const previousFocus = document.activeElement;
      content.showPopover();
      // Native dialog popovers can focus their first child before the toggle event fires.
      if (!focus && previousFocus instanceof HTMLElement) {
        previousFocus.focus({ preventScroll: true });
      }
    }
  }, []);
  const registerContent = useCallback((content: HTMLDialogElement | null) => {
    contentRef.current = content;
  }, []);
  const registerTrigger = useCallback((trigger: HTMLButtonElement | null) => {
    triggerRef.current = trigger;
  }, []);
  const handleToggle = useCallback(
    (content: HTMLDialogElement, nextOpen: boolean) => {
      setOpen(nextOpen);
      if (nextOpen && focusOnOpen.current) {
        content
          .querySelector<HTMLElement>(
            'button:not([disabled]), select:not([disabled]), input:not([disabled]), a[href], [tabindex]:not([tabindex="-1"])',
          )
          ?.focus();
      } else if (
        !nextOpen &&
        (content.contains(document.activeElement) || document.activeElement === document.body)
      ) {
        triggerRef.current?.focus();
      }
      focusOnOpen.current = true;
      onOpenChange?.(nextOpen);
    },
    [onOpenChange],
  );
  const value = useMemo(
    () => ({ id, open, close, show, registerContent, registerTrigger, handleToggle }),
    [id, open, close, show, registerContent, registerTrigger, handleToggle],
  );
  return <ActionPopoverContext value={value}>{children}</ActionPopoverContext>;
}

/** Consumers may open without moving focus on hover or close after performing an action. */
export function useActionPopover() {
  const { close, open, show } = useActionPopoverContext();
  return { close, open, show };
}

/** A normal identity button tracks disclosure only, never persistent selection. */
export function ActionPopoverTrigger({
  onClick,
  size = "icon",
  variant = size === "icon" ? "ghost" : "quiet",
  ref,
  ...props
}: ActionPopoverTriggerProps) {
  const { id, open, registerTrigger } = useActionPopoverContext();
  const triggerRef = useCallback(
    (trigger: HTMLButtonElement | null) => {
      registerTrigger(trigger);
      const cleanup = assignRef(ref, trigger);
      return () => {
        registerTrigger(null);
        if (typeof cleanup === "function") {
          cleanup();
        } else {
          assignRef(ref, null);
        }
      };
    },
    [registerTrigger, ref],
  );
  return (
    <Button
      {...props}
      ref={triggerRef}
      variant={variant}
      size={size}
      aria-haspopup="dialog"
      aria-controls={id}
      aria-expanded={open}
      popoverTarget={id}
      onClick={(event) => {
        registerTrigger(event.currentTarget);
        onClick?.(event);
      }}
    />
  );
}

/** A quiet chevron communicates disclosure and follows the root's native open state. */
export function ActionPopoverIndicator() {
  const { open } = useActionPopoverContext();
  return (
    <span
      aria-hidden="true"
      className={cn(
        "flex shrink-0 text-text-muted transition-transform duration-150 ease-swarmie motion-reduce:transition-none",
        open ? "rotate-180" : "rotate-0",
      )}
    >
      <Icon name="chevron" className="size-4" />
    </span>
  );
}

/** Consumers place the neutral panel; shared appearance and native focus behavior stay here. */
export function ActionPopoverContent({
  className,
  ref,
  onToggle,
  ...props
}: ActionPopoverContentProps) {
  const { id, registerContent, handleToggle } = useActionPopoverContext();
  const contentRef = useCallback(
    (content: HTMLDialogElement | null) => {
      registerContent(content);
      const cleanup = assignRef(ref, content);
      return () => {
        registerContent(null);
        if (typeof cleanup === "function") {
          cleanup();
        } else {
          assignRef(ref, null);
        }
      };
    },
    [registerContent, ref],
  );
  return (
    <dialog
      {...props}
      ref={contentRef}
      id={id}
      popover="auto"
      className={cn(
        "fixed m-0 max-h-[calc(100dvh-24px)] w-[min(256px,calc(100vw-68px))] overflow-y-auto overscroll-contain rounded-panel border-0 bg-surface p-1 text-ui text-ink shadow-overlay",
        className,
      )}
      onToggle={(event) => {
        event.stopPropagation();
        handleToggle(event.currentTarget, event.newState === "open");
        onToggle?.(event);
      }}
    />
  );
}

/** Ordinary buttons retain Tab navigation, native refs, and a compact rows with coarse-pointer touch targets. */
export function ActionPopoverItem({ className, ...props }: ActionPopoverItemProps) {
  return <Button {...props} variant="quiet" size="menu" className={cn("w-full", className)} />;
}
