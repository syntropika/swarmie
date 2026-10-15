import * as Primitive from "@radix-ui/react-switch";
import { cn } from "cn";
import { useId } from "react";

export function Switch({
  label,
  checked,
  onChange,
  disabled,
  hideLabel = false,
}: {
  label: string;
  checked: boolean;
  onChange: (value: boolean) => void;
  disabled?: boolean;
  hideLabel?: boolean;
}) {
  const id = useId();
  return (
    <div className="workspace-switch flex items-center gap-2 text-sm">
      <Primitive.Root
        id={id}
        checked={checked}
        onCheckedChange={onChange}
        disabled={disabled}
        aria-label={label}
        className="bg-control-hover data-[state=checked]:bg-accent focus-visible:ring-focus inline-flex h-4 w-7 shrink-0 items-center rounded-full p-0.5 transition-colors outline-none focus-visible:ring-2 disabled:opacity-40 motion-reduce:transition-none"
      >
        <Primitive.Thumb className="block size-3 rounded-full bg-white transition-transform data-[state=checked]:translate-x-3 motion-reduce:transition-none" />
      </Primitive.Root>
      <label
        htmlFor={id}
        className={cn(hideLabel && "sr-only", disabled === true && !hideLabel && "opacity-40")}
      >
        {label}
      </label>
    </div>
  );
}
