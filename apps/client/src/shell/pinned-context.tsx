import { useSource } from "@swarmie/client-ui";
import { Code2, Paperclip, X } from "lucide-react";

import { useClient } from "../client/context";
import { IconButton, Item, useChat } from "../controls";

export function PinnedContext() {
  const { openTab, shell, plugins, shellState } = useClient();
  const layout = useSource(shell);
  const installed = useSource(plugins);
  const chat = useChat();
  const draft = useSource(chat.draft);
  if (!layout.summary || layout.section !== "conversations") {
    return null;
  }
  return (
    <aside
      aria-label="Pinned context"
      className="absolute top-2 right-2 z-30 w-72 max-w-[calc(100vw-24px)] rounded-xl border border-border bg-overlay p-4 shadow-lg"
    >
      <div className="mb-4 flex items-center justify-between text-xs">
        <span className="text-muted">{chat.title}</span>
        <IconButton
          icon={X}
          label="Close pinned context"
          onPress={() => {
            shellState.update((current) => ({ ...current, summary: false }));
          }}
        />
      </div>
      <p className="mb-2 text-chrome text-muted">Sources</p>
      {draft.attachments.length === 0 ? (
        <p className="text-xs text-muted">No sources attached.</p>
      ) : (
        draft.attachments.map((name) => (
          <p key={name} className="flex items-center gap-2 py-2 text-xs">
            <Paperclip className="size-3.5" />
            {name}
          </p>
        ))
      )}
      {installed.some((plugin) => plugin.id === "app-state" && plugin.enabled) ? (
        <Item
          icon={Code2}
          onClick={() => {
            openTab("state");
          }}
        >
          Live application state
        </Item>
      ) : null}
    </aside>
  );
}
