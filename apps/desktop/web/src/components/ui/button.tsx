import type { ComponentProps } from "react";
import { cn } from "cn";

type ButtonProps = ComponentProps<"button"> & {
  variant?: "default" | "destructive" | "outline" | "secondary" | "ghost" | "link";
  size?: "default" | "xs" | "sm" | "lg" | "icon" | "icon-xs" | "icon-sm" | "icon-lg";
};
const variants = {
  default: "bg-control-primary text-control-primary-foreground hover:bg-white",
  destructive: "bg-danger text-white hover:brightness-110",
  outline: "border border-border bg-transparent hover:bg-control-hover",
  secondary: "bg-control-secondary text-foreground hover:bg-control-hover",
  ghost: "bg-transparent text-foreground hover:bg-[var(--button-bg-hover,var(--color-control-hover))] active:bg-[var(--button-bg-pressed,var(--color-control-hover))]",
  link: "bg-transparent text-foreground underline-offset-4 hover:underline",
};

// Native DOM handlers and refs preserve assistant-ui's send and cancel slots.
export function Button({ className, variant = "default", size = "default", type = "button", ...props }: ButtonProps) {
  const icon = size.startsWith("icon") || className?.split(/\s+/).includes("icon-button");
  const small = ["xs", "sm", "icon-xs", "icon-sm"].includes(size);
  return <button {...props} type={type} data-slot="button" data-variant={variant} data-size={size}
    className={cn("swarmie-button inline-flex shrink-0 items-center justify-center gap-1.5 rounded-lg text-sm font-medium whitespace-nowrap transition-colors outline-none focus-visible:ring-2 focus-visible:ring-focus focus-visible:ring-offset-2 focus-visible:ring-offset-background disabled:pointer-events-none disabled:opacity-40 motion-reduce:transition-none", variants[variant],
      small ? "h-7" : size === "lg" || size === "icon-lg" ? "h-10" : "h-8",
      icon ? small ? "w-7 p-0" : "w-8 p-0" : small ? "px-2.5" : "px-3", className)} />;
}
