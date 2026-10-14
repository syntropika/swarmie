import type { ComponentPropsWithRef } from "react";

import { Icon } from "./icon";

export type GlobeIconProps = ComponentPropsWithRef<"svg">;

/** A decorative globe for controls whose adjacent text supplies the accessible name. */
export function GlobeIcon(props: GlobeIconProps) {
  return <Icon {...props} name="world" />;
}
