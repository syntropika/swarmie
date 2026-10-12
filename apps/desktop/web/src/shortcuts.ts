export const defaultShortcuts = {
  palette: "Mod+K",
  newChat: "Mod+N",
  files: "Mod+P",
  tools: "Mod+Shift+E",
};
export type Shortcuts = typeof defaultShortcuts;
export function readShortcuts(): Shortcuts {
  try {
    return {
      ...defaultShortcuts,
      ...JSON.parse(localStorage.getItem("swarmie.shortcuts") ?? "{}"),
    };
  } catch {
    return { ...defaultShortcuts };
  }
}
export function matchesShortcut(
  event: KeyboardEvent,
  binding: string,
): boolean {
  const parts = binding.toLowerCase().split("+");
  const key = parts.pop();
  return (
    !!key &&
    event.key.toLowerCase() === key &&
    (event.ctrlKey || event.metaKey) === parts.includes("mod") &&
    event.shiftKey === parts.includes("shift") &&
    event.altKey === parts.includes("alt")
  );
}
export function shortcutBinding(
  event: Pick<
    KeyboardEvent,
    "key" | "ctrlKey" | "metaKey" | "shiftKey" | "altKey"
  >,
) {
  if (["Control", "Meta", "Shift", "Alt"].includes(event.key)) return;
  if (!event.ctrlKey && !event.metaKey && !event.altKey)
    throw new Error("Use Command, Control or Alt with a key.");
  return [
    ...(event.ctrlKey || event.metaKey ? ["Mod"] : []),
    ...(event.shiftKey ? ["Shift"] : []),
    ...(event.altKey ? ["Alt"] : []),
    event.key.toUpperCase(),
  ].join("+");
}
