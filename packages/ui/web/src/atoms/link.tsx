import { Link as HeroLink } from "@heroui/react";
import type { LinkProps as HeroLinkProps, LinkIconProps } from "@heroui/react";

import { cn } from "../classes";

export type LinkProps = HeroLinkProps;
/** Text navigation uses the vendor link and changes foreground on hover. */
function Root({ className, ...props }: LinkProps) {
  return (
    <HeroLink
      {...props}
      className={
        typeof className === "function"
          ? (state) => cn("swarmie-text-link", className(state))
          : cn("swarmie-text-link", className)
      }
    />
  );
}

// oxlint-disable-next-line no-redeclare -- The vendor API pairs a value with its compound prop type namespace.
export const Link = Object.assign(Root, { Root, Icon: HeroLink.Icon });

export { Root as LinkRoot };
export { LinkIcon } from "@heroui/react";
export type LinkRootProps = LinkProps;
export type Link = { Props: LinkProps; RootProps: LinkRootProps; IconProps: LinkIconProps };
