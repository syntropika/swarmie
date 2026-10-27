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
  variant: _variant,
  size: _size,
  ...props
}: TooltipIconButtonProps) {
  return (
    <Tooltip>
      <Button {...props} aria-label={tooltip} variant="tertiary" isIconOnly />
      <Tooltip.Content placement={side}>{tooltip}</Tooltip.Content>
    </Tooltip>
  );
}
