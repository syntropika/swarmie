import type { ComponentProps } from "react";

import { Button } from "../atoms/button";
import { Tooltip } from "../atoms/tooltip";

export type TooltipIconButtonProps = Omit<ComponentProps<typeof Button>, "variant" | "size"> & {
  tooltip: string;
  side?: "top" | "bottom" | "left" | "right";
  variant?: "ghost" | "outline" | "default";
  size?: "icon" | "sm";
};
export function TooltipIconButton({
  tooltip,
  side = "bottom",
  variant = "ghost",
  size = "icon",
  ...props
}: TooltipIconButtonProps) {
  const variants = { default: "primary", outline: "outline", ghost: "ghost" } as const;
  return (
    <Tooltip>
      <Button
        {...props}
        aria-label={tooltip}
        variant={variants[variant]}
        size={size === "sm" ? "sm" : "md"}
        isIconOnly
      />
      <Tooltip.Content placement={side}>{tooltip}</Tooltip.Content>
    </Tooltip>
  );
}
