import * as Primitive from "@radix-ui/react-slider";
import { cn } from "cn";
import type { ComponentProps } from "react";

export function Slider({
  label,
  valueText,
  children,
  className,
  ...props
}: ComponentProps<typeof Primitive.Root> & { label: string; valueText?: string }) {
  return (
    <Primitive.Root
      {...props}
      className={cn("relative flex h-7 touch-none items-center select-none", className)}
    >
      <Primitive.Track className="bg-control-hover relative h-6 w-full grow rounded-full">
        <Primitive.Range className="bg-accent absolute h-full rounded-full" />
        {children}
      </Primitive.Track>
      <Primitive.Thumb
        aria-label={label}
        aria-valuetext={valueText}
        className="focus-visible:ring-focus block size-7 rounded-full bg-white shadow-sm outline-none focus-visible:ring-2 data-[disabled]:opacity-40"
      />
    </Primitive.Root>
  );
}
