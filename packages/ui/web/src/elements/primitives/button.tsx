import type { ComponentProps, ComponentPropsWithRef } from "react";

import { Button as SharedButton } from "../../atoms/button";

export function Button({
  variant,
  size,
  type = "button",
  title,
  tabIndex,
  render,
  ...props
}: Omit<ComponentProps<typeof SharedButton>, "variant" | "size"> & {
  variant?: "ghost" | "outline" | "default" | "secondary" | undefined;
  size?: "icon" | "icon-sm" | "sm" | "default" | undefined;
} & Pick<ComponentPropsWithRef<"button">, "title" | "tabIndex">) {
  const variants = {
    default: "primary",
    ghost: "ghost",
    outline: "outline",
    secondary: "secondary",
  } as const;
  const controlVariant = variants[variant ?? "default"];
  return (
    <SharedButton
      {...props}
      variant={controlVariant}
      type={type}
      render={(nativeProps, state) => {
        const forwarded = {
          ...nativeProps,
          title: title ?? nativeProps.title,
          tabIndex: tabIndex ?? nativeProps.tabIndex,
        };
        if (render) {
          return render(forwarded, state);
        }
        if (type === "submit") {
          return <button {...forwarded} type="submit" />;
        }
        if (type === "reset") {
          return <button {...forwarded} type="reset" />;
        }
        return <button {...forwarded} type="button" />;
      }}
      {...(size === "icon" || size === "icon-sm" ? { isIconOnly: true } : {})}
      {...(size === "sm" || size === "icon-sm" ? { size: "sm" } : {})}
    />
  );
}
export function buttonVariants(options?: { variant?: string; size?: string }) {
  const variant = options?.variant;
  let color = "button--ghost";
  if (variant === "default") {
    color = "button--primary";
  } else if (variant === "outline") {
    color = "button--outline";
  } else if (variant === "secondary") {
    color = "button--secondary";
  }
  const compact = options?.size === "sm" || options?.size === "icon-sm";
  const iconOnly = options?.size === "icon" || options?.size === "icon-sm";
  return [
    "button",
    color,
    compact ? "button--sm" : "button--md",
    iconOnly ? "button--icon-only" : "",
  ]
    .filter(Boolean)
    .join(" ");
}
