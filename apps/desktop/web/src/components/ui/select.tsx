import * as Primitive from "@radix-ui/react-select";
import { Check, ChevronDown } from "lucide-react";
import { useId, type ReactNode } from "react";
import { cn } from "cn";
import { floatingClasses, itemClasses } from "./floating";

export function Select({ label, value, onChange, options, disabled, className = "", popoverClassName = "", placeholder = "Select…", icon }: {
  label: string; value: string; onChange: (value: string) => void; options: { id: string; label: string; disabled?: boolean; icon?: ReactNode }[];
  disabled?: boolean; className?: string; popoverClassName?: string; placeholder?: string; icon?: ReactNode;
}) {
  const defaultChoice = `mpai-default-${useId()}`;
  const selected = options.find((option) => option.id === value);
  return <Primitive.Root value={selected ? value || defaultChoice : ""} onValueChange={(next) => onChange(next === defaultChoice ? "" : next)} disabled={disabled}>
    <Primitive.Trigger aria-label={label} className={cn("workspace-select flex h-8 min-w-0 items-center justify-between gap-2 rounded-lg border border-border bg-control-secondary px-2.5 text-sm text-foreground outline-none hover:bg-control-hover focus-visible:ring-2 focus-visible:ring-focus disabled:opacity-40", className)}>
      <Primitive.Value placeholder={placeholder}>{selected && <span className="flex min-w-0 items-center gap-2 truncate">{icon}{selected.label}</span>}</Primitive.Value>
      <Primitive.Icon><ChevronDown size={14} /></Primitive.Icon>
    </Primitive.Trigger>
    <Primitive.Portal><Primitive.Content position="popper" sideOffset={8} collisionPadding={20} aria-label={label}
      className={cn(floatingClasses, "min-w-[var(--radix-select-trigger-width)] max-h-[var(--radix-select-content-available-height)] p-1.5", popoverClassName)}>
      <Primitive.Viewport>{options.map((option) => <Primitive.Item key={option.id} value={option.id || defaultChoice} disabled={option.disabled} textValue={option.label} className={itemClasses}>
        {option.icon}<Primitive.ItemText>{option.label}</Primitive.ItemText><Primitive.ItemIndicator className="ml-auto"><Check size={14} /></Primitive.ItemIndicator>
      </Primitive.Item>)}</Primitive.Viewport>
    </Primitive.Content></Primitive.Portal>
  </Primitive.Root>;
}
