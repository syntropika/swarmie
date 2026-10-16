// Adapted from the HeroUI Apache-2.0 documentation example: https://raw.githubusercontent.com/heroui-inc/heroui/7a6cbbd6f6622f2562d26349f4728033b626706b/apps/docs/src/demos/en/label/basic.tsx
import { Input, Label } from "../../src/index";

export function OfficialExample() {
  return (
    <div className="flex flex-col gap-1">
      <Label htmlFor="name">Name</Label>
      <Input className="w-64" id="name" placeholder="Enter your name" type="text" />
    </div>
  );
}
