// Adapted from the HeroUI Apache-2.0 documentation example: https://raw.githubusercontent.com/heroui-inc/heroui/7a6cbbd6f6622f2562d26349f4728033b626706b/apps/docs/src/demos/en/textarea/basic.tsx
import { TextArea } from "../../src/index";

export function OfficialExample() {
  return (
    <TextArea
      aria-label="Quick project update"
      className="h-32 w-96"
      placeholder="Share a quick project update..."
    />
  );
}
