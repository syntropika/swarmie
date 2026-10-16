// Adapted from the HeroUI Apache-2.0 documentation example: https://raw.githubusercontent.com/heroui-inc/heroui/7a6cbbd6f6622f2562d26349f4728033b626706b/apps/docs/src/demos/en/color-field/basic.tsx
"use client";

import { useState } from "react";

import type { Color } from "../../src/index";
import { ColorField, ColorSwatch, Label, parseColor } from "../../src/index";

export function OfficialExample() {
  const [color, setColor] = useState<Color | null>(parseColor("#0485F7"));

  return (
    <ColorField className="w-[280px]" name="color" value={color} onChange={setColor}>
      <Label>Color</Label>
      <ColorField.Group>
        <ColorField.Prefix>
          <ColorSwatch {...(color === null ? {} : { color })} size="xs" />
        </ColorField.Prefix>
        <ColorField.Input />
      </ColorField.Group>
    </ColorField>
  );
}
