import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import type { WorkspaceStore } from "./workspace";

export function ChatDirectorySettings({ store, directory, custom, connected }: {
  store: WorkspaceStore;
  directory: string;
  custom: boolean;
  connected: boolean;
}) {
  const [path, setPath] = useState(directory);
  const [saving, setSaving] = useState(false);
  useEffect(() => setPath(directory), [directory]);
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
  return <section className="chat-directory-settings">
    <h3>New chats</h3>
    <label>Chat directory<Input value={path} onChange={(event) => setPath(event.target.value)} placeholder="Documents folder" disabled={!connected || saving} /></label>
    <p className="muted">Chats without a project start here. Existing chats keep their current folder.</p>
    <div className="chat-directory-actions">
      <Button variant="secondary" disabled={!connected || saving || !path.trim() || path === directory} onClick={() => void save(path)}>{saving ? "Saving…" : "Save directory"}</Button>
      <Button variant="ghost" disabled={!connected || saving || !custom} onClick={() => void save()}>Use Documents</Button>
    </div>
  </section>;
}
