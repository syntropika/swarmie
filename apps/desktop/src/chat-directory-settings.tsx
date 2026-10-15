import { useId, useState } from "react";

import { startAction } from "~/src/action-boundary";
import { Button } from "~/src/components/ui/button";
import { Input } from "~/src/components/ui/input";
import type { WorkspaceStore } from "~/src/workspace";

export function ChatDirectorySettings({
  store,
  directory,
  custom,
  connected,
}: {
  store: WorkspaceStore;
  directory: string;
  custom: boolean;
  connected: boolean;
}) {
  const fieldId = useId();
  const [draft, setDraft] = useState({ directory, path: directory });
  const path = draft.directory === directory ? draft.path : directory;
  const [saving, setSaving] = useState(false);
  const save = async (value?: string) => {
    setSaving(true);
    try {
      await store.setChatDirectory(value);
    } catch (error) {
      store.setError(error);
    } finally {
      setSaving(false);
    }
  };
  return (
    <section className="chat-directory-settings">
      <h3>New chats</h3>
      <label htmlFor={fieldId}>
        Chat directory
        <Input
          id={fieldId}
          value={path}
          onChange={(event) => {
            setDraft({ directory, path: event.target.value });
          }}
          placeholder="Documents folder"
          disabled={!connected || saving}
        />
      </label>
      <p className="muted">
        Chats without a project start here. Existing chats keep their current folder.
      </p>
      <div className="chat-directory-actions">
        <Button
          variant="secondary"
          disabled={!connected || saving || !path.trim() || path === directory}
          onClick={() => {
            startAction(save(path), store.setError.bind(store));
          }}
        >
          {saving ? "Saving…" : "Save directory"}
        </Button>
        <Button
          variant="ghost"
          disabled={!connected || saving || !custom}
          onClick={() => {
            startAction(save(), store.setError.bind(store));
          }}
        >
          Use Documents
        </Button>
      </div>
    </section>
  );
}
