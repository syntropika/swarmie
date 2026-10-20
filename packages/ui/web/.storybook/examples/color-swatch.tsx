// Adapted from the HeroUI Apache-2.0 documentation example: https://raw.githubusercontent.com/heroui-inc/heroui/7a6cbbd6f6622f2562d26349f4728033b626706b/apps/docs/src/demos/en/color-swatch/basic.tsx
import { ColorSwatch } from "../../src/index";

export function OfficialExample() {
  return (
    <div className="flex items-center gap-3">
      <ColorSwatch aria-label="Blue" color="#0485F7" />
      <ColorSwatch aria-label="Red" color="#EF4444" />
      <ColorSwatch aria-label="Amber" color="#F59E0B" />
      <ColorSwatch aria-label="Green" color="#10B981" />
      <ColorSwatch aria-label="Fuchsia" color="#D946EF" />
    </div>
  );
}
