import {
  ArrowDownUp,
  Archive,
  Pencil,
  Share,
  ExternalLink,
  Copy,
  Cloud,
  Plane,
  Calendar,
  ThumbsUp,
  ThumbsDown,
  Smile,
  Laugh,
  Undo2,
  Ellipsis,
  Volume2,
  GitFork,
  Flag,
  Monitor,
  CircleAlert,
  TriangleAlert,
  ArrowLeft,
  ArrowRight,
  ArrowLeftRight,
  AtSign,
  Bell,
  BriefcaseBusiness,
  Check,
  ChevronDown,
  CircleCheck,
  CircleX,
  Clock,
  Eye,
  EyeOff,
  FileText,
  Folder,
  History,
  House,
  Sun,
  Puzzle,
  Store,
  Inbox,
  KeyRound,
  LayoutGrid,
  PanelLeft,
  PanelRight,
  LockKeyhole,
  LogOut,
  Mail,
  Menu,
  MessageCircle,
  Mic,
  Pin,
  Plus,
  Bot,
  Search,
  Settings,
  Square,
  ListChecks,
  LoaderCircle,
  Layers,
  Users,
  Globe,
  X,
  Activity,
  Slash,
  ArrowUp,
  RefreshCw,
  Command,
  Paperclip,
  Maximize2,
  Minimize2,
  CircleHelp,
  LogIn,
} from "lucide-react";
import type { ComponentPropsWithRef } from "react";

import { cn } from "../classes";

// Keep the catalog bounded: named imports allow unused provider icons to be removed at build time.
const icons = {
  activity: Activity,
  slash: Slash,
  "arrow-up": ArrowUp,
  refresh: RefreshCw,
  command: Command,
  paperclip: Paperclip,
  expand: Maximize2,
  contract: Minimize2,
  help: CircleHelp,
  login: LogIn,
  archive: Archive,
  rename: Pencil,
  share: Share,
  external: ExternalLink,
  copy: Copy,
  cloud: Cloud,
  plane: Plane,
  calendar: Calendar,
  "thumbs-up": ThumbsUp,
  "thumbs-down": ThumbsDown,
  smile: Smile,
  "smile-wink": Laugh,
  reply: Undo2,
  dots: Ellipsis,
  volume: Volume2,
  fork: GitFork,
  flag: Flag,
  screen: Monitor,
  agent: Bot,
  arrow: ArrowRight,
  "arrows-exchange": ArrowLeftRight,
  back: ArrowLeft,
  bell: Bell,
  chat: MessageCircle,
  check: Check,
  chevron: ChevronDown,
  "circle-attention": CircleAlert,
  "circle-check": CircleCheck,
  "circle-close": CircleX,
  clock: Clock,
  close: X,
  eye: Eye,
  "eye-off": EyeOff,
  file: FileText,
  grid: LayoutGrid,
  history: History,
  home: House,
  appearance: Sun,
  plugin: Puzzle,
  marketplace: Store,
  inbox: Inbox,
  key: KeyRound,
  lock: LockKeyhole,
  logout: LogOut,
  mail: Mail,
  memory: Layers,
  mention: AtSign,
  menu: Menu,
  microphone: Mic,
  panel: PanelRight,
  people: Users,
  pin: Pin,
  plus: Plus,
  project: Folder,
  search: Search,
  settings: Settings,
  sidebar: PanelLeft,
  sort: ArrowDownUp,
  stop: Square,
  task: ListChecks,
  loading: LoaderCircle,
  "triangle-attention": TriangleAlert,
  workspaces: BriefcaseBusiness,
  world: Globe,
};

/** Semantic names for the shared outline icon catalog. */
export type IconName = keyof typeof icons;

/** Enumerate the shared icon contract for pickers and the foundation catalog. */
export const iconNames = Object.keys(icons).filter((name): name is keyof typeof icons =>
  Object.hasOwn(icons, name),
);

/** Lucide SVG attributes and refs; the adjacent control or text supplies the accessible name. */
export type IconProps = ComponentPropsWithRef<"svg"> & {
  name: IconName;
  /** Selected interface indicators may use the shared brand accent. */
  tone?: "default" | "brand";
};

/** Lucide outline geometry on a 24-unit grid, rendered at 20px with a consistent 1.6-unit stroke. */
export function Icon({
  name,
  tone = "default",
  className,
  stroke = "currentColor",
  strokeWidth = "var(--stroke-chrome-icon, 1.6)",
  ...props
}: IconProps) {
  const Glyph = icons[name];

  return (
    <Glyph
      size={20}
      color={stroke}
      strokeWidth={strokeWidth}
      {...props}
      aria-hidden="true"
      focusable="false"
      className={cn("size-5 shrink-0", tone === "brand" && "text-brand-accent", className)}
    />
  );
}
