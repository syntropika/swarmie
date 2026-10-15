import { cva } from "class-variance-authority";
import type { VariantProps } from "class-variance-authority";
import { cn } from "cn";
import type { ComponentProps } from "react";

const buttonVariants = cva(
  "swarmie-button focus-visible:ring-focus focus-visible:ring-offset-background inline-flex shrink-0 items-center justify-center gap-1.5 rounded-lg text-sm font-medium whitespace-nowrap transition-colors outline-none focus-visible:ring-2 focus-visible:ring-offset-2 disabled:pointer-events-none disabled:opacity-40 motion-reduce:transition-none",
  {
    variants: {
      variant: {
        section: "text-muted gap-1 bg-transparent px-0 text-xs font-medium hover:bg-transparent",
        thread: "thread-row text-foreground hover:bg-control-hover bg-transparent",
        navigation: "sidebar-nav-item text-foreground hover:bg-control-hover bg-transparent",
        wordmark: "workspace-wordmark text-foreground hover:bg-control-hover bg-transparent",
        backdrop: "sidebar-backdrop bg-transparent",
        default: "bg-control-primary text-control-primary-foreground hover:bg-white",
        destructive: "bg-danger text-white hover:brightness-110",
        outline: "hover:bg-control-hover border border-border bg-transparent",
        secondary: "bg-control-secondary text-foreground hover:bg-control-hover",
        ghost: "text-foreground hover:bg-control-hover active:bg-control-hover bg-transparent",
        link: "text-foreground bg-transparent underline-offset-4 hover:underline",
        composer:
          "text-foreground bg-transparent hover:bg-composer-cap-hover active:bg-composer-cap-hover",
        reveal: "scroll-bottom text-foreground hover:bg-control-hover bg-transparent",
        command: "command-result text-foreground hover:bg-control-hover bg-transparent",
        file: "file-review-name text-foreground hover:bg-control-hover bg-transparent",
        send: "bg-control-primary text-control-primary-foreground hover:bg-white",
        stop: "bg-control-primary text-control-primary-foreground hover:bg-white",
      },
      size: {
        default: "h-8 px-3",
        xs: "h-7 px-2.5",
        sm: "h-7 px-2.5",
        lg: "h-10 px-3",
        icon: "h-8 w-8 p-0",
        "icon-xs": "h-7 w-7 p-0",
        "icon-sm": "h-7 w-7 p-0",
        "icon-lg": "h-10 w-8 p-0",
      },
      shape: { default: "", pill: "rounded-full" },
      tone: {
        default: "",
        muted: "text-muted hover:text-foreground",
        success: "text-accent hover:text-accent",
      },
      selected: { true: "selected", false: "" },
    },
    compoundVariants: [{ variant: "section", class: "gap-1 px-0 text-xs hover:bg-transparent" }],
    defaultVariants: {
      variant: "default",
      size: "default",
      shape: "default",
      tone: "default",
      selected: false,
    },
  },
);

type ButtonProps = ComponentProps<"button"> & VariantProps<typeof buttonVariants>;

// Native props and refs preserve assistant-ui's send and cancel slots.
export function Button({ className, variant, size, shape, tone, selected, ...props }: ButtonProps) {
  const { type, ...nativeProps } = props;
  const attributes = {
    ...nativeProps,
    "data-slot": "button",
    "data-variant": variant,
    "data-size": size,
    className: cn(buttonVariants({ variant, size, shape, tone, selected }), className),
  };
  if (type === "submit") {
    return <button {...attributes} type="submit" />;
  }
  if (type === "reset") {
    return <button {...attributes} type="reset" />;
  }
  return <button {...attributes} type="button" />;
}
