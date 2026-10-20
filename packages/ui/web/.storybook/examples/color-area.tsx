// Adapted from the HeroUI Apache-2.0 documentation example: https://raw.githubusercontent.com/heroui-inc/heroui/7a6cbbd6f6622f2562d26349f4728033b626706b/apps/docs/src/demos/en/color-area/basic.tsx
import { ColorArea } from "../../src/index";

export function OfficialExample() {
  return (
    <ColorArea defaultValue="rgb(116, 52, 255)">
      <ColorArea.Thumb />
    </ColorArea>
  );
}
