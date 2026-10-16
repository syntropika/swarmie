// Adapted from the HeroUI Apache-2.0 documentation example: https://raw.githubusercontent.com/heroui-inc/heroui/7a6cbbd6f6622f2562d26349f4728033b626706b/apps/docs/src/demos/en/toggle-button/basic.tsx
import { Heart } from "@gravity-ui/icons";

import { ToggleButton } from "../../src/index";

export function OfficialExample() {
  return (
    <ToggleButton>
      <Heart />
      Like
    </ToggleButton>
  );
}
