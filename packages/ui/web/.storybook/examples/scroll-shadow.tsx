// Adapted from the HeroUI Apache-2.0 documentation example: https://raw.githubusercontent.com/heroui-inc/heroui/7a6cbbd6f6622f2562d26349f4728033b626706b/apps/docs/src/demos/en/scroll-shadow/default.tsx
import { ScrollShadow } from "../../src/index";

export function OfficialExample() {
  return (
    <div className="w-full p-0 sm:max-w-sm">
      <ScrollShadow className="max-h-[240px] p-4">
        <div className="space-y-4">
          {[
            "opening",
            "context",
            "details",
            "content",
            "examples",
            "notes",
            "summary",
            "references",
            "appendix",
            "closing",
          ].map((id) => (
            <p key={id}>
              Lorem ipsum dolor sit amet, consectetur adipiscing elit. Nullam pulvinar risus non
              risus hendrerit venenatis. Pellentesque sit amet hendrerit risus, sed porttitor quam.
              Morbi accumsan cursus enim, sed ultricies sapien.
            </p>
          ))}
        </div>
      </ScrollShadow>
    </div>
  );
}
