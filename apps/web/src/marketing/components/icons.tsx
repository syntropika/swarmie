import { Icon } from "@swarmie/ui-web";
import type { ComponentPropsWithRef } from "react";

/** A directional cue for links to the next action. */
export function ArrowRight(props: ComponentPropsWithRef<"svg">) {
  return <Icon {...props} name="arrow" />;
}
