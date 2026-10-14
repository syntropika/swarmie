import * as TooltipPrimitive from "@radix-ui/react-tooltip";
import { createContext, useCallback, useContext, useMemo, useState } from "react";
import type { ComponentProps, ComponentPropsWithRef, Ref } from "react";

import { cn } from "../classes";

type TooltipContextValue = {
  container: HTMLElement | null;
  registerTrigger: (trigger: HTMLButtonElement | null) => void;
};

const TooltipContext = createContext<TooltipContextValue | undefined>(undefined);

function useTooltipContext() {
  const context = useContext(TooltipContext);
  if (context === undefined) {
    throw new Error("Tooltip parts require TooltipRoot.");
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

/** Shared delay policy; callers may opt into a longer discovery delay. */
export function TooltipProvider({
  delayDuration = 0,
  ...props
}: ComponentProps<typeof TooltipPrimitive.Provider>) {
  return <TooltipPrimitive.Provider {...props} delayDuration={delayDuration} />;
}

export function TooltipRoot({ children, ...props }: ComponentProps<typeof TooltipPrimitive.Root>) {
  const [container, setContainer] = useState<HTMLElement | null>(null);
  const registerTrigger = useCallback((trigger: HTMLButtonElement | null) => {
    setContainer(trigger?.closest<HTMLElement>("[data-theme]") ?? null);
  }, []);
  const context = useMemo(() => ({ container, registerTrigger }), [container, registerTrigger]);
  return (
    <TooltipContext value={context}>
      <TooltipPrimitive.Root {...props}>{children}</TooltipPrimitive.Root>
    </TooltipContext>
  );
}

/** The child retains its native button semantics and forwarded ref. */
export function TooltipTrigger({
  ref,
  ...props
}: ComponentPropsWithRef<typeof TooltipPrimitive.Trigger>) {
  const { registerTrigger } = useTooltipContext();
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
  return <TooltipPrimitive.Trigger {...props} ref={triggerRef} />;
}

/** Portal positioning handles viewport collisions without clipping the explanation. */
export function TooltipContent({
  className,
  sideOffset = 8,
  collisionPadding = 8,
  ...props
}: ComponentPropsWithRef<typeof TooltipPrimitive.Content>) {
  const { container } = useTooltipContext();
  return (
    <TooltipPrimitive.Portal container={container}>
      <TooltipPrimitive.Content
        {...props}
        sideOffset={sideOffset}
        collisionPadding={collisionPadding}
        className={cn(
          "z-50 max-w-[min(320px,calc(100vw-16px))] origin-(--radix-tooltip-content-transform-origin) rounded-control bg-inverse px-3 py-1.5 text-caption leading-ui text-inverse-text shadow-overlay motion-safe:animate-tooltip-open data-[state=closed]:motion-safe:animate-tooltip-close motion-reduce:animate-none",
          className,
        )}
      />
    </TooltipPrimitive.Portal>
  );
}
