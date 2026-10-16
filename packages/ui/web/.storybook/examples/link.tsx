// Adapted from the HeroUI Apache-2.0 documentation example: https://raw.githubusercontent.com/heroui-inc/heroui/7a6cbbd6f6622f2562d26349f4728033b626706b/apps/docs/src/demos/en/link/basic.tsx
import { Link } from "../../src/index";

export function OfficialExample() {
  return (
    <Link href="#">
      Call to action
      <Link.Icon />
    </Link>
  );
}
