import { Command } from "cmdk";
import * as Primitive from "@radix-ui/react-popover";
import { ChevronDown } from "lucide-react";
import { useRef, useState } from "react";
import { Button } from "./button";
import { fieldClasses } from "./input";
import { floatingClasses, itemClasses } from "./floating";
import { cn } from "cn";

export function ComboBox({ label, value, onChange, options, placeholder, disabled }: {
  label: string; value: string; onChange: (value: string) => void; options: string[]; placeholder?: string; disabled?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const input = useRef<HTMLInputElement>(null);
  return <Primitive.Root open={open} onOpenChange={setOpen}><Command label={label} loop>
    <Primitive.Anchor asChild><div className="workspace-combobox relative">
      <Command.Input ref={input} aria-label={label} value={value} onValueChange={onChange} placeholder={placeholder} disabled={disabled} aria-expanded={open}
        onKeyDown={(event) => { if (event.key === "ArrowDown" && !open) { event.preventDefault(); setOpen(true); } }}
        className={cn(fieldClasses, "h-9 w-full pr-9")} />
      <Primitive.Trigger asChild><Button variant="ghost" size="icon-sm" disabled={disabled} aria-label={`Show ${label.toLowerCase()} suggestions`} className="absolute top-1 right-1"><ChevronDown size={14} /></Button></Primitive.Trigger>
    </div></Primitive.Anchor>
    {open && <Primitive.Portal><Primitive.Content sideOffset={8} align="start" collisionPadding={20}
      onOpenAutoFocus={(event) => { event.preventDefault(); input.current?.focus(); }}
      onInteractOutside={(event) => { if (event.target === input.current) event.preventDefault(); }}
      onCloseAutoFocus={(event) => { event.preventDefault(); input.current?.focus(); }}
      className={cn(floatingClasses, "w-[var(--radix-popover-trigger-width)] min-w-48 p-1.5")}>
      <Command.List aria-label={`${label} suggestions`} className="max-h-60 overflow-auto">
        {options.map((option) => <Command.Item key={option} value={option} onSelect={() => { onChange(option); setOpen(false); }} className={cn(itemClasses, "data-[selected=true]:bg-control-hover")}>{option}</Command.Item>)}
        <Command.Empty className="px-2 py-1.5 text-sm text-muted">No matching suggestions. You can use the typed reference.</Command.Empty>
      </Command.List>
    </Primitive.Content></Primitive.Portal>}
  </Command></Primitive.Root>;
}
