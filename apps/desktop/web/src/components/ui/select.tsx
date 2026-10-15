import * as Primitive from "@radix-ui/react-select";
import { cva } from "class-variance-authority";
import { cn } from "cn";
import { Check, ChevronDown } from "lucide-react";
import { useId } from "react";
import type { ReactNode } from "react";

import { floatingClasses, itemClasses } from "~/web/src/components/ui/floating";

const selectVariants = cva(
  "workspace-select text-foreground focus-visible:ring-focus flex h-8 min-w-0 items-center justify-between gap-2 rounded-lg px-2.5 text-sm outline-none focus-visible:ring-2 disabled:opacity-40",
  {
    variants: {
      variant: {
        default: "bg-control-secondary hover:bg-control-hover border border-border",
        composer:
          "rounded-full border border-transparent bg-transparent hover:bg-composer-cap-hover",
      },
    },
    defaultVariants: { variant: "default" },
  },
);
export function Select({
  label,
  variant = "default",
  id,
  value,
  onChange,
  options,
  disabled,
  className = "",
  popoverClassName = "",
  placeholder = "Select…",
  icon,
}: {
  label: string;
  variant?: "default" | "composer";
  id?: string;
  value: string;
  onChange: (value: string) => void;
  options: { id: string; label: string; disabled?: boolean; icon?: ReactNode }[];
  disabled?: boolean;
  className?: string;
  popoverClassName?: string;
  placeholder?: string;
  icon?: ReactNode;
}) {
  const defaultChoice = `swarmie-default-${useId()}`;
  const selected = options.find((option) => option.id === value);
  return (
    <Primitive.Root
      value={selected ? value || defaultChoice : ""}
      onValueChange={(next) => {
        onChange(next === defaultChoice ? "" : next);
      }}
      disabled={disabled}
    >
      <Primitive.Trigger
        id={id}
        aria-label={label}
        className={cn(selectVariants({ variant }), className)}
      >
        <span className="min-w-0 flex-1 overflow-hidden text-left">
          <Primitive.Value placeholder={placeholder}>
            {selected && (
              <span className="flex w-full min-w-0 items-center gap-2">
                {icon !== undefined && icon !== null && <span className="shrink-0">{icon}</span>}
                <span className="min-w-0 flex-1 truncate">{selected.label}</span>
              </span>
            )}
          </Primitive.Value>
        </span>
        <Primitive.Icon className="shrink-0">
          <ChevronDown size={14} />
        </Primitive.Icon>
      </Primitive.Trigger>
      <Primitive.Portal>
        <Primitive.Content
          position="popper"
          sideOffset={8}
          collisionPadding={20}
          aria-label={label}
          className={cn(
            floatingClasses,
            "max-h-[var(--radix-select-content-available-height)] min-w-[var(--radix-select-trigger-width)] p-1.5",
            popoverClassName,
          )}
        >
          <Primitive.Viewport>
            {options.map((option) => (
              <Primitive.Item
                key={option.id}
                value={option.id || defaultChoice}
                disabled={option.disabled}
                textValue={option.label}
                className={itemClasses}
              >
                {option.icon}
                <Primitive.ItemText>{option.label}</Primitive.ItemText>
                <Primitive.ItemIndicator className="ml-auto">
                  <Check size={14} />
                </Primitive.ItemIndicator>
              </Primitive.Item>
            ))}
          </Primitive.Viewport>
        </Primitive.Content>
      </Primitive.Portal>
    </Primitive.Root>
  );
}
