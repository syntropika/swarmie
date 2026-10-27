import type { ComponentProps } from "react";

import { Button as SharedButton } from "../../atoms/button";

export function Button({
  variant,
  size,
  ...props
}: Omit<ComponentProps<typeof SharedButton>, "variant" | "size"> & {
  variant?: "ghost" | "outline" | "default" | "secondary" | undefined;
  size?: "icon" | "icon-sm" | "sm" | "default" | undefined;
}) {
  const variants = {
    default: "primary",
    ghost: "tertiary",
    outline: "outline",
    secondary: "secondary",
  } as const;
  const controlVariant = variants[variant ?? "default"];
  return (
    <SharedButton
      {...props}
      variant={controlVariant}
      {...(size === "icon" || size === "icon-sm" ? { isIconOnly: true } : {})}
      {...(size === "sm" ? { size: "sm" } : {})}
    />
  );
}
export function buttonVariants(options?: { variant?: string; size?: string }) {
  if (options?.variant === "default") {
    return "button button--primary";
  }
  return "button button--tertiary";
}
