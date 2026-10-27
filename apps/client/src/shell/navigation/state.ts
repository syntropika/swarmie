import type { Shell } from "../../client/projections";

export type NavigationPreferences = {
  mode: "chats" | "notifications";
  folded: readonly string[];
  setMode: (mode: "chats" | "notifications") => void;
  toggleGroup: (id: string) => void;
};

export const shellSection = (shell: Shell) => shell.section;
export const shellSettings = (shell: Shell) => shell.settings;
export const activeConversation = (shell: Shell) => shell.active;
export const navigationLocation = ({ navigation, section }: Shell) => ({ navigation, section });
export const sameNavigationLocation = (
  a: ReturnType<typeof navigationLocation>,
  b: ReturnType<typeof navigationLocation>,
) => a.navigation === b.navigation && a.section === b.section;
