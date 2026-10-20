// Adapted from the HeroUI Apache-2.0 documentation example: https://raw.githubusercontent.com/heroui-inc/heroui/7a6cbbd6f6622f2562d26349f4728033b626706b/apps/docs/src/demos/en/badge/basic.tsx
import { Avatar, Badge } from "../../src/index";

const GREEN_AVATAR_URL = "https://heroui-assets.nyc3.cdn.digitaloceanspaces.com/avatars/green.jpg";
const ORANGE_AVATAR_URL =
  "https://heroui-assets.nyc3.cdn.digitaloceanspaces.com/avatars/orange.jpg";
const BLUE_AVATAR_URL = "https://heroui-assets.nyc3.cdn.digitaloceanspaces.com/avatars/blue.jpg";

export function OfficialExample() {
  return (
    <div className="flex items-center gap-6">
      <Badge.Anchor>
        <Avatar>
          <Avatar.Image src={GREEN_AVATAR_URL} />
          <Avatar.Fallback>JD</Avatar.Fallback>
        </Avatar>
        <Badge color="danger" size="sm">
          5
        </Badge>
      </Badge.Anchor>

      <Badge.Anchor>
        <Avatar>
          <Avatar.Image src={ORANGE_AVATAR_URL} />
          <Avatar.Fallback>AB</Avatar.Fallback>
        </Avatar>
        <Badge color="accent" size="sm">
          New
        </Badge>
      </Badge.Anchor>

      <Badge.Anchor>
        <Avatar>
          <Avatar.Image src={BLUE_AVATAR_URL} />
          <Avatar.Fallback>CD</Avatar.Fallback>
        </Avatar>
        <Badge color="success" placement="bottom-right" size="sm" />
      </Badge.Anchor>
    </div>
  );
}
