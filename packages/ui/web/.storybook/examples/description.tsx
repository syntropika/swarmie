// Adapted from the HeroUI Apache-2.0 documentation example: https://raw.githubusercontent.com/heroui-inc/heroui/7a6cbbd6f6622f2562d26349f4728033b626706b/apps/docs/src/demos/en/description/basic.tsx
import { Description, Input, Label } from "../../src/index";

export function OfficialExample() {
  return (
    <div className="flex flex-col gap-1">
      <Label htmlFor="email">Email</Label>
      <Input
        aria-describedby="email-description"
        className="w-64"
        id="email"
        placeholder="you@example.com"
        type="email"
      />
      <Description id="email-description">
        We&apos;ll never share your email with anyone else.
      </Description>
    </div>
  );
}
