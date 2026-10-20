// Adapted from the HeroUI Apache-2.0 documentation example: https://raw.githubusercontent.com/heroui-inc/heroui/7a6cbbd6f6622f2562d26349f4728033b626706b/apps/docs/src/demos/en/textfield/basic.tsx
import { Input, Label, TextField } from "../../src/index";

export function OfficialExample() {
  return (
    <TextField className="w-full max-w-64" name="email" type="email">
      <Label>Email</Label>
      <Input placeholder="Enter your email" />
    </TextField>
  );
}
