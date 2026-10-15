import { Icon, cn } from "@swarmie/ui-web";
import type { IconName } from "@swarmie/ui-web";

export type LoginIconName =
  | "arrow"
  | "mail"
  | "key"
  | "lock"
  | "eye"
  | "eye-off"
  | "check"
  | "back"
  | "attention";

const icons = {
  arrow: "arrow",
  back: "back",
  mail: "mail",
  key: "key",
  lock: "lock",
  eye: "eye",
  "eye-off": "eye-off",
  check: "circle-check",
  attention: "circle-attention",
} satisfies Record<LoginIconName, IconName>;

/** Identity feedback preserves its circled symbols within the shared outline catalog. */
export function LoginIcon({ name, className }: { name: LoginIconName; className?: string }) {
  return <Icon name={icons[name]} className={cn("basis-5", className)} />;
}
