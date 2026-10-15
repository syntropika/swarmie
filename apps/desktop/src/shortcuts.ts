import { isRecord } from "~/src/checked-json";
import { hasText } from "~/src/value-presence";

export const defaultShortcuts = {
  palette: "Mod+K",
  newChat: "Mod+N",
  files: "Mod+P",
  tools: "Mod+Shift+E",
};
export type Shortcuts = typeof defaultShortcuts;
export function readShortcuts(): Shortcuts {
  try {
    const saved: unknown = JSON.parse(localStorage.getItem("swarmie.shortcuts") ?? "{}");
    if (!isRecord(saved)) {
      return { ...defaultShortcuts };
    }
    return {
      palette: typeof saved.palette === "string" ? saved.palette : defaultShortcuts.palette,
      newChat: typeof saved.newChat === "string" ? saved.newChat : defaultShortcuts.newChat,
      files: typeof saved.files === "string" ? saved.files : defaultShortcuts.files,
      tools: typeof saved.tools === "string" ? saved.tools : defaultShortcuts.tools,
    };
  } catch {
    return { ...defaultShortcuts };
  }
}
export function matchesShortcut(
  event: Pick<KeyboardEvent, "key" | "ctrlKey" | "metaKey" | "shiftKey" | "altKey">,
  binding: string,
): boolean {
  const parts = binding.toLowerCase().split("+");
  const key = parts.pop();
  return (
    hasText(key) &&
    event.key.toLowerCase() === key &&
    (event.ctrlKey || event.metaKey) === parts.includes("mod") &&
    event.shiftKey === parts.includes("shift") &&
    event.altKey === parts.includes("alt")
  );
}
export function shortcutBinding(
  event: Pick<KeyboardEvent, "key" | "ctrlKey" | "metaKey" | "shiftKey" | "altKey">,
) {
  if (["Control", "Meta", "Shift", "Alt"].includes(event.key)) {
    return undefined;
  }
  if (!event.ctrlKey && !event.metaKey && !event.altKey) {
    throw new Error("Use Command, Control or Alt with a key.");
  }
  return [
    ...(event.ctrlKey || event.metaKey ? ["Mod"] : []),
    ...(event.shiftKey ? ["Shift"] : []),
    ...(event.altKey ? ["Alt"] : []),
    event.key.toUpperCase(),
  ].join("+");
}
