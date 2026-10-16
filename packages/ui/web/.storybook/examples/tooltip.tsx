// Adapted from the HeroUI Apache-2.0 documentation example: https://raw.githubusercontent.com/heroui-inc/heroui/7a6cbbd6f6622f2562d26349f4728033b626706b/apps/docs/src/demos/en/tooltip/basic.tsx
import { CircleInfo } from "@gravity-ui/icons";

import { Button, Tooltip } from "../../src/index";

export function OfficialExample() {
  return (
    <div className="flex items-center gap-4">
      <Tooltip delay={0}>
        <Button variant="secondary">Hover me</Button>
        <Tooltip.Content>
          <p>This is a tooltip</p>
        </Tooltip.Content>
      </Tooltip>

      <Tooltip delay={0}>
        <Button isIconOnly aria-label="More information" variant="tertiary">
          <CircleInfo />
        </Button>
        <Tooltip.Content>
          <p>More information</p>
        </Tooltip.Content>
      </Tooltip>
    </div>
  );
}
