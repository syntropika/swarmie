import * as Primitive from "@radix-ui/react-switch";
import { useId } from "react";

export function Switch({ label, checked, onChange, disabled }: { label: string; checked: boolean; onChange: (value: boolean) => void; disabled?: boolean }) {
  const id = useId();
  return <div className="workspace-switch flex items-center gap-2 text-sm">
    <Primitive.Root id={id} checked={checked} onCheckedChange={onChange} disabled={disabled} aria-label={label}
      className="inline-flex h-4 w-7 shrink-0 items-center rounded-full bg-control-hover p-0.5 outline-none transition-colors data-[state=checked]:bg-accent focus-visible:ring-2 focus-visible:ring-focus disabled:opacity-40 motion-reduce:transition-none">
      <Primitive.Thumb className="block size-3 rounded-full bg-white transition-transform data-[state=checked]:translate-x-3 motion-reduce:transition-none" />
    </Primitive.Root>
    <label htmlFor={id} className={disabled ? "opacity-40" : undefined}>{label}</label>
  </div>;
}
