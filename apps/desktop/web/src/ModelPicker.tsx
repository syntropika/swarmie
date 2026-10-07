import * as Primitive from "@radix-ui/react-select";
import { Check, ChevronRight } from "lucide-react";
import { useState } from "react";
import { Button } from "@/components/ui/button";
import { floatingClasses, itemClasses } from "@/components/ui/floating";
import { cn } from "cn";

export function ModelPicker({ choices, value, label, effort, disabled, onChange }: {
  choices: { id: string; label: string }[]; value: string; label: string; effort?: string; disabled: boolean;
  onChange: (value: string) => Promise<boolean>;
}) {
  const [open, setOpen] = useState(false);
  return <Primitive.Root open={open} onOpenChange={setOpen} value={value} onValueChange={(next) => {
    if (next !== value) void onChange(next).then((success) => { if (!success) setOpen(true); });
  }}>
    <Primitive.Trigger asChild><Button variant="secondary" size={effort ? "lg" : "sm"} className="min-w-0 max-w-full justify-self-center rounded-lg"
      aria-label={`Choose model: ${label}`} disabled={disabled || !choices.length}>
      <span className="flex min-w-0 flex-col items-center">
        {effort && <span className="text-[#aaa8ef] capitalize">{effort}</span>}
        <span className="flex max-w-full items-center gap-1 text-xs text-muted">{label}<ChevronRight size={12} /></span>
      </span>
    </Button></Primitive.Trigger>
    <Primitive.Portal><Primitive.Content position="popper" side="top" align="end" sideOffset={8} collisionPadding={20}
      aria-label="Available models" className={cn(floatingClasses, "w-80 max-h-[var(--radix-select-content-available-height)] p-1.5")}>
      <div className="px-2.5 py-1.5 text-sm text-muted">Select model</div>
      <Primitive.Viewport className="max-h-65 overflow-y-auto">
        {choices.map((choice) => <Primitive.Item key={choice.id} value={choice.id} textValue={choice.label} disabled={disabled} className={itemClasses}>
          <Primitive.ItemText>{choice.label}</Primitive.ItemText><Primitive.ItemIndicator className="ml-auto"><Check size={14} /></Primitive.ItemIndicator>
        </Primitive.Item>)}
      </Primitive.Viewport>
    </Primitive.Content></Primitive.Portal>
  </Primitive.Root>;
}
