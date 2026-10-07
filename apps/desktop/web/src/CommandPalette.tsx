import { useState } from "react";
import { Dialog } from "@/components/ui/dialog";
import { CommandPalette as Palette, type PaletteCommand } from "@/components/assistant-ui/elements/command-palette";
import type { WorkspaceStore } from "./workspace";
import { hasConversation } from "./protocol";

export function CommandPalette({ store, onClose, onPanel, onOpenProject }: {
  store: WorkspaceStore;
  onClose: () => void;
  onOpenProject: () => void;
  onPanel: (panel: "project" | "settings" | "review" | "extensions") => void;
}) {
  const [query, setQuery] = useState("");
  const [activeId, setActiveId] = useState("action:new");
  const actions = [
    { id: "action:new", label: "New chat", run: () => store.createThread() },
    { id: "action:projects", label: "Open project", run: onOpenProject },
    { id: "action:tools", label: "Workspace tools", run: () => onPanel("extensions") },
    { id: "action:review", label: "Review changes", run: () => { onPanel("review"); void store.review().catch((error) => store.setError(error)); } },
    { id: "action:connection", label: "Connection settings", run: () => onPanel("settings") },
    ...store.getSnapshot().threads.filter(hasConversation).map((thread) => ({ id: `chat:${thread.id}`, label: `${thread.archived ? "Archived · " : ""}${thread.title}`, run: () => store.select(thread.id) })),
  ];
  const commands: PaletteCommand[] = actions.map((action) => ({ id: action.id, label: action.label, group: action.id.startsWith("chat:") ? "Conversations" : "Actions", keys: [] }));
  return <Dialog className="max-w-sm bg-transparent p-0 shadow-none" label="Command palette" onClose={onClose}>
    <Palette commands={commands} query={query} activeId={activeId}
      onQueryChange={(next) => { setQuery(next); setActiveId(commands.find((command) => command.label.toLowerCase().includes(next.toLowerCase()))?.id ?? ""); }}
      onActiveChange={setActiveId} onRun={(id) => { const action = actions.find((entry) => entry.id === id); if (action) { onClose(); action.run(); } }} />
  </Dialog>;
}
