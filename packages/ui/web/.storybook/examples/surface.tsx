// Adapted from the HeroUI Apache-2.0 documentation example: https://raw.githubusercontent.com/heroui-inc/heroui/7a6cbbd6f6622f2562d26349f4728033b626706b/apps/docs/src/demos/en/surface/basic.tsx
import { Surface } from "../../src/index";

export function OfficialExample() {
  return (
    <Surface className="flex min-w-[320px] flex-col gap-3 rounded-3xl p-6" variant="default">
      <h3 className="text-base font-semibold text-foreground">Surface Content</h3>
      <p className="text-sm text-muted">
        This is a default surface variant. It uses bg-surface styling.
      </p>
    </Surface>
  );
}
