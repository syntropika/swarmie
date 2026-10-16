// Adapted from the HeroUI Apache-2.0 documentation example: https://raw.githubusercontent.com/heroui-inc/heroui/7a6cbbd6f6622f2562d26349f4728033b626706b/apps/docs/src/demos/en/number-field/basic.tsx
import { Label, NumberField } from "../../src/index";

export function OfficialExample() {
  return (
    <NumberField className="w-full max-w-64" defaultValue={1024} minValue={0} name="width">
      <Label>Width</Label>
      <NumberField.Group>
        <NumberField.DecrementButton />
        <NumberField.Input className="w-[120px]" />
        <NumberField.IncrementButton />
      </NumberField.Group>
    </NumberField>
  );
}
