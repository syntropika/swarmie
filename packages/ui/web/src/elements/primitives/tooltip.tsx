import type { ComponentProps, PropsWithChildren, ReactElement, ReactNode } from "react";

import { Tooltip as SharedTooltip } from "../../atoms/tooltip";
import { ElementButton } from "../controls";

export function TooltipProvider({ children }: PropsWithChildren): ReactNode {
  return children;
}
export { Tooltip } from "../../atoms/tooltip";
export function TooltipTrigger({
  render,
  children,
}: {
  render: ReactElement<ComponentProps<typeof ElementButton>>;
  children?: React.ReactNode;
}) {
  return <ElementButton {...render.props}>{children}</ElementButton>;
}
export function TooltipContent({
  side = "top",
  sideOffset,
  ...props
}: Omit<ComponentProps<typeof SharedTooltip.Content>, "placement"> & {
  side?: "top" | "bottom" | "left" | "right" | undefined;
  sideOffset?: number;
}) {
  return (
    <SharedTooltip.Content
      {...props}
      placement={side}
      {...(sideOffset === undefined ? {} : { offset: sideOffset })}
    />
  );
}
