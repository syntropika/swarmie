import { Settings, Bell, Keyboard, Plug, UserRound, ShieldCheck, Terminal } from "lucide-react";

export const settingsSections = [
  { id: "general", title: "General", icon: Settings, group: "Personal" },
  { id: "account", title: "Account", icon: UserRound, group: "Personal" },
  { id: "security", title: "Account security", icon: ShieldCheck, group: "Personal" },
  { id: "agents", title: "Harnesses", icon: Terminal, group: "Coding" },
  { id: "notifications", title: "Notifications", icon: Bell, group: "Personal" },
  { id: "shortcuts", title: "Keyboard shortcuts", icon: Keyboard, group: "Personal" },
  { id: "mcps", title: "MCP servers", icon: Plug, group: "Integrations" },
  { id: "extensions", title: "Extensions", icon: Plug, group: "Integrations" },
  { id: "connections", title: "Nodes", icon: Plug, group: "Coding" },
] as const;

export type SettingsSection = (typeof settingsSections)[number]["id"];

export function visibleSettingsSections(signedIn: boolean) {
  return settingsSections.filter((section) => section.id !== "security" || signedIn);
}
