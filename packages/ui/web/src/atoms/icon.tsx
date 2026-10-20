import {
  IconCopy,
  IconCloud,
  IconPlane,
  IconCalendar,
  IconThumbUp,
  IconThumbDown,
  IconMoodSmile,
  IconMoodWink,
  IconArrowBackUp,
  IconDots,
  IconVolume,
  IconGitFork,
  IconFlag,
  IconDeviceDesktop,
  IconAlertCircle,
  IconAlertTriangle,
  IconArrowLeft,
  IconArrowRight,
  IconArrowsExchange,
  IconAt,
  IconBell,
  IconBriefcase,
  IconCheck,
  IconChevronDown,
  IconCircleCheck,
  IconCircleX,
  IconClock,
  IconEye,
  IconEyeOff,
  IconFileText,
  IconFolder,
  IconHistory,
  IconHome,
  IconInbox,
  IconKey,
  IconLayoutGrid,
  IconLayoutSidebar,
  IconLayoutSidebarRight,
  IconLock,
  IconLogout,
  IconMail,
  IconMenu2,
  IconMessage,
  IconMicrophone,
  IconPin,
  IconPlus,
  IconRobot,
  IconSearch,
  IconSettings,
  IconSquare,
  IconListCheck,
  IconLoader2,
  IconStack,
  IconUsers,
  IconWorld,
  IconX,
} from "@tabler/icons-react";
import type { ComponentPropsWithRef } from "react";

import { cn } from "../classes";

// Keep the catalog bounded: named imports allow unused provider icons to be removed at build time.
const icons = {
  copy: IconCopy,
  cloud: IconCloud,
  plane: IconPlane,
  calendar: IconCalendar,
  "thumbs-up": IconThumbUp,
  "thumbs-down": IconThumbDown,
  smile: IconMoodSmile,
  "smile-wink": IconMoodWink,
  reply: IconArrowBackUp,
  dots: IconDots,
  volume: IconVolume,
  fork: IconGitFork,
  flag: IconFlag,
  screen: IconDeviceDesktop,
  agent: IconRobot,
  arrow: IconArrowRight,
  "arrows-exchange": IconArrowsExchange,
  back: IconArrowLeft,
  bell: IconBell,
  chat: IconMessage,
  check: IconCheck,
  chevron: IconChevronDown,
  "circle-attention": IconAlertCircle,
  "circle-check": IconCircleCheck,
  "circle-close": IconCircleX,
  clock: IconClock,
  close: IconX,
  eye: IconEye,
  "eye-off": IconEyeOff,
  file: IconFileText,
  grid: IconLayoutGrid,
  history: IconHistory,
  home: IconHome,
  inbox: IconInbox,
  key: IconKey,
  lock: IconLock,
  logout: IconLogout,
  mail: IconMail,
  memory: IconStack,
  mention: IconAt,
  menu: IconMenu2,
  microphone: IconMicrophone,
  panel: IconLayoutSidebarRight,
  people: IconUsers,
  pin: IconPin,
  plus: IconPlus,
  project: IconFolder,
  search: IconSearch,
  settings: IconSettings,
  sidebar: IconLayoutSidebar,
  stop: IconSquare,
  task: IconListCheck,
  loading: IconLoader2,
  "triangle-attention": IconAlertTriangle,
  workspaces: IconBriefcase,
  world: IconWorld,
};

/** Semantic names for the shared outline icon catalog. */
export type IconName = keyof typeof icons;

/** Enumerate the shared icon contract for pickers and the foundation catalog. */
export const iconNames = Object.keys(icons).filter((name): name is keyof typeof icons =>
  Object.hasOwn(icons, name),
);

/** Tabler SVG attributes and refs; the adjacent control or text supplies the accessible name. */
export type IconProps = ComponentPropsWithRef<"svg"> & { name: IconName };

/** Tabler outline geometry on a 24-unit grid, rendered at 20px with a consistent 1.5-unit stroke. */
export function Icon({
  name,
  className,
  stroke = "currentColor",
  strokeWidth = 1.5,
  ...props
}: IconProps) {
  const Glyph = icons[name];

  return (
    <Glyph
      size={20}
      color={stroke}
      stroke={strokeWidth}
      {...props}
      aria-hidden="true"
      className={cn("size-5 shrink-0", className)}
    />
  );
}
