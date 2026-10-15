import { useState, useSyncExternalStore } from "react";

import { startAction } from "~/web/src/action-boundary";
import { CommandPalette as Palette } from "~/web/src/components/assistant-ui/elements/command-palette";
import type { PaletteCommand } from "~/web/src/components/assistant-ui/elements/command-palette";
import { Dialog } from "~/web/src/components/ui/dialog";
import { useContributions } from "~/web/src/contribution-host";
import { hasConversation } from "~/web/src/protocol";
import type { WorkspaceStore } from "~/web/src/workspace";

export function CommandPalette({
  store,
  onClose,
  onPanel,
  onOpenProject,
}: {
  store: WorkspaceStore;
  onClose: () => void;
  onOpenProject: () => void;
  onPanel: (panel: "project" | "settings" | "review" | "extensions") => void;
}) {
  const host = useContributions();
  useSyncExternalStore(
    host?.registry.subscribe ?? subscribeWithoutHost,
    host?.registry.getSnapshot ?? emptySnapshot,
  );
  const [query, setQuery] = useState("");
  const [activeId, setActiveId] = useState("action:new");
  const actions = [
    ...(host?.registry.commands() ?? []).map((command) => ({
      id: command.key,
      label: command.title,
      run: () => {
        host?.registry.run(command.key);
      },
    })),
    {
      id: "action:new",
      label: "New chat",
      run: () => {
        store.createThread();
      },
    },
    { id: "action:projects", label: "Open project", run: onOpenProject },
    {
      id: "action:tools",
      label: "Workspace tools",
      run: () => {
        onPanel("extensions");
      },
    },
    {
      id: "action:review",
      label: "Review changes",
      run: () => {
        onPanel("review");
        startAction(store.review(), store.setError.bind(store));
      },
    },
    {
      id: "action:connection",
      label: "Connection settings",
      run: () => {
        onPanel("settings");
      },
    },
    ...store
      .getSnapshot()
      .threads.filter(hasConversation)
      .map((thread) => ({
        id: `chat:${thread.id}`,
        label: `${thread.archived === true ? "Archived · " : ""}${thread.title}`,
        run: () => {
          store.select(thread.id);
        },
      })),
  ];
  const commands: PaletteCommand[] = actions.map((action) => ({
    id: action.id,
    label: action.label,
    group: action.id.startsWith("chat:") ? "Conversations" : "Actions",
    keys: [],
  }));
  return (
    <Dialog
      variant="command-palette"
      className="max-w-sm"
      label="Command palette"
      onClose={onClose}
    >
      <Palette
        commands={commands}
        query={query}
        activeId={activeId}
        onQueryChange={(next) => {
          setQuery(next);
          setActiveId(
            commands.find((command) => command.label.toLowerCase().includes(next.toLowerCase()))
              ?.id ?? "",
          );
        }}
        onActiveChange={setActiveId}
        onRun={(id) => {
          const action = actions.find((entry) => entry.id === id);
          if (action) {
            onClose();
            action.run();
          }
        }}
      />
    </Dialog>
  );
}

function subscribeWithoutHost() {
  return unsubscribeWithoutHost;
}
function unsubscribeWithoutHost() {
  /* No registry was acquired, so there is no subscription to release. */
}
function emptySnapshot() {
  return 0;
}
