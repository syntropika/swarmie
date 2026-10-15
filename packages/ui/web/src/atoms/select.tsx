import * as SelectPrimitive from "@radix-ui/react-select";
import { cva } from "class-variance-authority";
import type { VariantProps } from "class-variance-authority";
import { createContext, useCallback, useContext, useId, useMemo, useState } from "react";
import type { ComponentPropsWithRef, ReactNode, Ref } from "react";

import { cn } from "../classes";
import { Icon } from "./icon";

const triggerVariants = cva(
  "group inline-flex min-h-control min-w-0 items-center justify-between gap-2 rounded-pill border px-3 py-1.5 text-ui leading-ui font-medium transition-colors duration-150 ease-swarmie focus-visible:outline-2 focus-visible:outline-offset-2 disabled:cursor-not-allowed disabled:opacity-50 aria-invalid:border-danger aria-invalid:focus-visible:outline-danger motion-reduce:transition-none pointer-coarse:min-h-control-touch",
  {
    variants: {
      variant: { surface: "", ghost: "border-transparent bg-transparent" },
      tone: {
        default: "text-ink focus-visible:outline-primary",
        inverse: "text-inverse-text focus-visible:outline-inverse-text",
      },
    },
    compoundVariants: [
      {
        variant: "surface",
        tone: "default",
        className:
          "border-transparent bg-surface-muted enabled:hover:bg-surface-hover data-[state=open]:bg-surface-hover",
      },
      {
        variant: "surface",
        tone: "inverse",
        className:
          "border-transparent bg-transparent enabled:hover:bg-inverse/10 data-[state=open]:bg-inverse/10",
      },
      {
        variant: "ghost",
        tone: "default",
        className:
          "enabled:hover:bg-surface-hover enabled:hover:text-primary data-[state=open]:bg-surface-hover data-[state=open]:text-primary",
      },
      {
        variant: "ghost",
        tone: "inverse",
        className: "enabled:hover:bg-inverse/10 data-[state=open]:bg-inverse/10",
      },
    ],
    defaultVariants: { variant: "surface", tone: "default" },
  },
);

export type SelectRootProps = ComponentPropsWithRef<typeof SelectPrimitive.Root>;

export type SelectTriggerProps = Omit<
  ComponentPropsWithRef<typeof SelectPrimitive.Trigger>,
  "asChild"
> &
  VariantProps<typeof triggerVariants> & { controlSize?: "default" | "compact" };

export type SelectValueProps = ComponentPropsWithRef<typeof SelectPrimitive.Value>;

export type SelectContentProps = Omit<
  ComponentPropsWithRef<typeof SelectPrimitive.Content>,
  "asChild" | "position"
>;

export type SelectItemProps = Omit<
  ComponentPropsWithRef<typeof SelectPrimitive.Item>,
  "asChild"
> & {
  description?: ReactNode;
  leading?: ReactNode;
};

export type SelectGroupProps = ComponentPropsWithRef<typeof SelectPrimitive.Group>;

export type SelectLabelProps = ComponentPropsWithRef<typeof SelectPrimitive.Label>;

type SelectContextValue = {
  container: HTMLElement | null;
  registerTrigger: (trigger: HTMLButtonElement | null) => void;
};

const SelectContext = createContext<SelectContextValue | undefined>(undefined);

function useSelectContext() {
  const context = useContext(SelectContext);
  if (!context) {
    throw new Error("Select parts require SelectRoot.");
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

/** Own the value and open state here; option labels and application behavior stay with callers. */
export function SelectRoot({ children, ...props }: SelectRootProps) {
  const [container, setContainer] = useState<HTMLElement | null>(null);
  const registerTrigger = useCallback((trigger: HTMLButtonElement | null) => {
    setContainer(trigger?.closest<HTMLElement>("[data-theme]") ?? null);
  }, []);
  const context = useMemo(() => ({ container, registerTrigger }), [container, registerTrigger]);
  return (
    <SelectContext value={context}>
      <SelectPrimitive.Root {...props}>{children}</SelectPrimitive.Root>
    </SelectContext>
  );
}

/** Group related choices under an accessible provider or category label. */
export const SelectGroup = SelectPrimitive.Group;

export function SelectLabel({ className, ...props }: SelectLabelProps) {
  return (
    <SelectPrimitive.Label
      {...props}
      className={cn("px-2 pt-2 pb-1 text-xs font-medium text-text-muted", className)}
    />
  );
}

/** Pass an explicit child for a selected label that must also appear in server-rendered HTML. */
export const SelectValue = SelectPrimitive.Value;

/** Compose an optional leading icon and SelectValue; the trigger supplies the disclosure icon. */
export function SelectTrigger({
  className,
  variant,
  tone,
  controlSize = "default",
  children,
  ref,
  ...props
}: SelectTriggerProps) {
  const { registerTrigger } = useSelectContext();
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
    <SelectPrimitive.Trigger
      {...props}
      ref={triggerRef}
      className={cn(
        triggerVariants({ variant, tone }),
        controlSize === "compact" && "min-h-control-compact px-2 py-0",
        className,
      )}
    >
      {children}
      <SelectPrimitive.Icon asChild>
        <Icon
          name="chevron"
          className="ml-auto size-3.5 shrink-0 transition-transform duration-150 ease-swarmie group-data-[state=open]:rotate-180 motion-reduce:transition-none"
        />
      </SelectPrimitive.Icon>
    </SelectPrimitive.Trigger>
  );
}

/** Portal the options above surrounding layout and constrain them to the available viewport. */
export function SelectContent({
  className,
  children,
  sideOffset = 4,
  collisionPadding = 12,
  ...props
}: SelectContentProps) {
  const { container } = useSelectContext();
  return (
    <SelectPrimitive.Portal container={container ?? undefined}>
      <SelectPrimitive.Content
        {...props}
        position="popper"
        sideOffset={sideOffset}
        collisionPadding={collisionPadding}
        className={cn(
          "swarmie-select-popup z-50 max-h-(--radix-select-content-available-height) max-w-(--radix-select-content-available-width) min-w-[max(12rem,var(--radix-select-trigger-width))] overflow-hidden rounded-panel border-0 bg-surface p-0 font-sans text-ui text-ink shadow-overlay",
          className,
        )}
      >
        <SelectPrimitive.ScrollUpButton className="flex h-7 items-center justify-center bg-surface text-text-muted">
          <Icon name="chevron" className="size-4 rotate-180" />
        </SelectPrimitive.ScrollUpButton>
        <SelectPrimitive.Viewport className="p-1">{children}</SelectPrimitive.Viewport>
        <SelectPrimitive.ScrollDownButton className="flex h-7 items-center justify-center bg-surface text-text-muted">
          <Icon name="chevron" className="size-4" />
        </SelectPrimitive.ScrollDownButton>
      </SelectPrimitive.Content>
    </SelectPrimitive.Portal>
  );
}

/** Keep the selected text, disabled semantics, typeahead, and visible checkmark together. */
export function SelectItem({
  className,
  children,
  description,
  leading,
  ...props
}: SelectItemProps) {
  const descriptionId = useId();
  return (
    <SelectPrimitive.Item
      {...props}
      aria-describedby={
        description === undefined
          ? props["aria-describedby"]
          : [props["aria-describedby"], descriptionId].filter(Boolean).join(" ")
      }
      className={cn(
        "relative flex min-h-control-small cursor-default items-center gap-2 rounded-control py-1 pr-8 pl-2 leading-ui font-medium transition-colors duration-100 ease-swarmie outline-none select-none data-[disabled]:pointer-events-none data-[disabled]:text-text-muted data-[disabled]:opacity-60 data-[highlighted]:bg-primary-soft data-[highlighted]:text-primary data-[state=checked]:font-medium data-[state=checked]:text-primary motion-reduce:transition-none pointer-coarse:min-h-control-touch",
        className,
      )}
    >
      {leading === undefined ? null : (
        <span aria-hidden="true" className="shrink-0">
          {leading}
        </span>
      )}
      <span className="min-w-0 flex-1">
        <SelectPrimitive.ItemText>{children}</SelectPrimitive.ItemText>
        {description === undefined ? null : (
          <span id={descriptionId} className="block text-xs leading-4 font-normal text-text-muted">
            {description}
          </span>
        )}
      </span>
      <SelectPrimitive.ItemIndicator className="absolute right-2 flex size-3.5 items-center justify-center text-primary">
        <Icon name="check" className="size-3.5" />
      </SelectPrimitive.ItemIndicator>
    </SelectPrimitive.Item>
  );
}
