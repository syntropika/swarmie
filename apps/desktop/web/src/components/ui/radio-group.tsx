import * as Primitive from "@radix-ui/react-radio-group";
import { useId } from "react";

export const RadioGroup = Primitive.Root;
export function RadioOption({ value, label }: { value: string; label: string }) {
  const id = useId();
  return (
    <div className="flex items-center gap-2 py-1 text-sm">
      <Primitive.Item
        id={id}
        value={value}
        aria-label={label}
        className="border-muted data-[state=checked]:border-accent focus-visible:ring-focus flex size-4 items-center justify-center rounded-full border outline-none focus-visible:ring-2 disabled:opacity-40"
      >
        <Primitive.Indicator className="bg-accent size-2 rounded-full" />
      </Primitive.Item>
      <label htmlFor={id}>{label}</label>
    </div>
  );
}
