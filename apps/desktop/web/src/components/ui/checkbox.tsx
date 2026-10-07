import * as Primitive from "@radix-ui/react-checkbox";
import { Check } from "lucide-react";
import { useId } from "react";

export function Checkbox({ label, checked, onChange, disabled, hideLabel = false, className = "" }: {
  label: string; checked: boolean; onChange: (checked: boolean) => void; disabled?: boolean; hideLabel?: boolean; className?: string;
}) {
  const id = useId();
  return <div className={`workspace-checkbox flex items-center gap-2 text-sm ${className}`}>
    <Primitive.Root id={id} checked={checked} onCheckedChange={(value) => onChange(value === true)} disabled={disabled} aria-label={label}
      className="flex size-4 shrink-0 items-center justify-center rounded-[4px] border border-muted bg-transparent outline-none data-[state=checked]:border-accent data-[state=checked]:bg-accent focus-visible:ring-2 focus-visible:ring-focus disabled:opacity-40">
      <Primitive.Indicator><Check size={12} className="text-white" /></Primitive.Indicator>
    </Primitive.Root>
    {!hideLabel && <label htmlFor={id} className={disabled ? "opacity-40" : undefined}>{label}</label>}
  </div>;
}
