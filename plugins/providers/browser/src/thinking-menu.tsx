import { ActionMenuDialog, Button, Popover, usePhoneLayout } from "@swarmie/ui-web";
import { ChevronRight } from "lucide-react";

import type { ProviderModel } from "./index";
import { ReasoningMark } from "./reasoning-mark";

/** Model capabilities are informational; reading them does not change the saved defaults. */
export function ThinkingMenu({ providerId, model }: { providerId: string; model: ProviderModel }) {
  const phone = usePhoneLayout();
  const [first] = model.efforts;
  const last = model.efforts.at(-1);
  if (first === undefined || last === undefined) {
    return <p className="text-xs text-muted">No configurable reasoning</p>;
  }
  const range = first === last ? first : `${first}–${last}`;
  return (
    <Popover>
      <Button
        size="sm"
        variant="secondary"
        aria-label={`${model.label}: Supported thinking`}
        className="max-w-full min-w-0 justify-self-start"
      >
        <ReasoningMark providerId={providerId} levels={model.efforts} value={last} />
        <span className="min-w-0 truncate">{range}</span>
        <ChevronRight />
      </Button>
      <Popover.Content
        placement={phone ? "bottom end" : "right top"}
        className="z-50 w-48 max-w-[calc(100vw-24px)]"
      >
        <ActionMenuDialog aria-label={`${model.label}: Supported thinking levels`}>
          <Popover.Heading>Supported thinking</Popover.Heading>
          <ul aria-label={`${model.label} thinking levels`}>
            {model.efforts.map((effort) => (
              <li
                key={effort}
                className="flex min-h-(--spacing-menu-row) items-center gap-(--spacing-menu-row-gap) px-(--spacing-menu-row-x) py-(--spacing-menu-row-y)"
              >
                <ReasoningMark providerId={providerId} levels={model.efforts} value={effort} />
                <span className="text-sm wrap-anywhere">{effort}</span>
              </li>
            ))}
          </ul>
        </ActionMenuDialog>
      </Popover.Content>
    </Popover>
  );
}
