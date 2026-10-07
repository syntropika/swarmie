import * as Primitive from "@radix-ui/react-radio-group";
import { useId } from "react";
export const RadioGroup = Primitive.Root;
export function RadioOption({ value, label }: { value: string; label: string }) {
  const id = useId();
  return <div className="flex items-center gap-2 py-1 text-sm"><Primitive.Item id={id} value={value} aria-label={label}
    className="flex size-4 items-center justify-center rounded-full border border-muted outline-none data-[state=checked]:border-accent focus-visible:ring-2 focus-visible:ring-focus disabled:opacity-40">
    <Primitive.Indicator className="size-2 rounded-full bg-accent" /></Primitive.Item><label htmlFor={id}>{label}</label></div>;
}
