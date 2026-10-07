import * as Primitive from "@radix-ui/react-slider";
import type { ComponentProps } from "react";
import { cn } from "cn";

export function Slider({ label, valueText, children, className, ...props }: ComponentProps<typeof Primitive.Root> & { label: string; valueText?: string }) {
  return <Primitive.Root {...props} className={cn("relative flex h-7 touch-none items-center select-none", className)}>
    <Primitive.Track className="relative h-6 w-full grow rounded-full bg-control-hover">
      <Primitive.Range className="absolute h-full rounded-full bg-accent" />{children}
    </Primitive.Track>
    <Primitive.Thumb aria-label={label} aria-valuetext={valueText}
      className="block size-7 rounded-full bg-white shadow-sm outline-none focus-visible:ring-2 focus-visible:ring-focus data-[disabled]:opacity-40" />
  </Primitive.Root>;
}
