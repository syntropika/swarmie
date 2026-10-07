import { useState } from "react";
import { Dropdown } from "@/components/ui/dropdown";
import { InputGroup } from "@/components/ui/input-group";
import { Dialog } from "@/components/ui/dialog";
import { ChevronDown, Folder, FolderPlus, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Checkbox } from "@/components/ui/checkbox";
import type { WorkspaceStore } from "./workspace";

type Source = "existing" | "create" | "clone";

export function ProjectDialog({ store, onClose }: { store: WorkspaceStore; onClose: () => void }) {
  const [name, setName] = useState("");
  const [folder, setFolder] = useState("");
  const [draftPath, setDraftPath] = useState("");
  const [source, setSource] = useState<Source>("existing");
  const [editing, setEditing] = useState(false);
  const [url, setUrl] = useState("");
  const [git, setGit] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const state = store.getSnapshot();
  const workspace = state.extensions.some((extension) => extension.id === "workspace");
  const enabled = state.connected && (state.git || workspace);
  const valid = name.trim() && (source === "existing" ? folder : draftPath.trim()) && (source !== "clone" || url.trim());

  const addFolder = () => {
    const path = draftPath.trim();
    if (!path) return;
    setFolder(path);
    if (!name.trim()) setName(path.split(/[\\/]/).filter(Boolean).at(-1) ?? path);
    setEditing(false);
    setError("");
  };
  const chooseSource = (next: Source) => {
    setSource(next);
    setEditing(true);
    setError("");
  };
  const submit = async () => {
    if (!valid || !enabled || busy) return;
    setBusy(true);
    setError("");
    try {
      let path = folder;
      if (source !== "existing") {
        const created = await store.invokeExtension<{ path: string }>("workspace", `project.${source}`, {
          parent: draftPath.trim(), name: name.trim(), url: url.trim(), initializeGit: git,
        });
        path = created.path;
      }
      const project = await store.openProject(path, name.trim());
      if (project) onClose();
      else setError(store.getSnapshot().error || "The folder could not be opened. Check its path and try again.");
    } catch (error) {
      setError(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog label="Create project" onClose={onClose} busy={busy} className="project-dialog">
            <form onSubmit={(event) => { event.preventDefault(); void submit(); }} aria-busy={busy}>
              <header className="project-dialog-header">
                <h2 className="text-xl font-semibold">Create project</h2>
                <Button variant="ghost" size="icon" aria-label="Close project dialog" disabled={busy} onClick={onClose}><X size={15} /></Button>
              </header>
              <div className="project-dialog-body">
                <InputGroup className="project-name-field" icon={<Folder size={15} />} autoFocus aria-label="Project name" placeholder="Project name" value={name} disabled={busy} onChange={(event) => setName(event.target.value)} />
                <label className="project-source-label" id="project-source-label">Source folders</label>
                <div className="project-source-box" role="group" aria-labelledby="project-source-label">
                  <Dropdown>
                    <Button variant="ghost" className="project-source-trigger" disabled={busy}>
                      {source === "existing" ? "Add a folder on this computer" : source === "create" ? "Create a new folder" : "Clone a repository"}<ChevronDown size={13} />
                    </Button>
                    <Dropdown.Popover className="workspace-menu">
                      <Dropdown.Menu aria-label="Project source">
                        <Dropdown.Item id="existing" onAction={() => chooseSource("existing")}>Add an existing folder</Dropdown.Item>
                        <Dropdown.Item id="create" isDisabled={!workspace} onAction={() => chooseSource("create")}>Create a new folder</Dropdown.Item>
                        <Dropdown.Item id="clone" isDisabled={!workspace} onAction={() => chooseSource("clone")}>Clone a repository</Dropdown.Item>
                      </Dropdown.Menu>
                    </Dropdown.Popover>
                  </Dropdown>
                  {source === "existing" && folder && !editing ? (
                    <div className="project-source-folder">
                      <Folder size={17} aria-hidden="true" />
                      <span title={folder}>{folder.split(/[\\/]/).filter(Boolean).at(-1)}<small>{folder}</small></span>
                      <Button variant="ghost" size="icon" aria-label="Remove source folder" disabled={busy} onClick={() => { setFolder(""); setDraftPath(""); }}><X size={14} /></Button>
                    </div>
                  ) : !editing ? (
                    <Button variant="secondary" className="project-add-folder" disabled={busy || !enabled} onClick={() => setEditing(true)}><FolderPlus size={14} />Add</Button>
                  ) : (
                    <div className="project-source-fields">
                      <label>{source === "existing" ? "Folder path" : "Parent folder"}
                        <Input autoFocus aria-label={source === "existing" ? "Folder path" : "Parent folder"} placeholder="Absolute directory path" value={draftPath} disabled={busy} onChange={(event) => setDraftPath(event.target.value)}
                          onKeyDown={(event) => { if (event.key === "Enter" && source === "existing") { event.preventDefault(); addFolder(); } }} />
                      </label>
                      {source === "existing" && <Button variant="secondary" disabled={busy || !draftPath.trim()} onClick={addFolder}>Add folder</Button>}
                      {source === "create" && <Checkbox label="Initialize a Git repository" checked={git} disabled={busy} onChange={setGit} />}
                      {source === "clone" && <label>Repository URL<Input aria-label="Repository URL" placeholder="Repository URL" value={url} disabled={busy} onChange={(event) => setUrl(event.target.value)} /></label>}
                    </div>
                  )}
                </div>
                {!enabled && <p className="project-dialog-error" role="alert">Connect to a daemon with the Workspace or Git extension to add a project.</p>}
                {error && <p className="project-dialog-error" role="alert">{error}</p>}
              </div>
              <footer className="project-dialog-footer">
                <Button variant="ghost" disabled={busy} onClick={onClose}>Cancel</Button>
                <Button type="submit" className="project-create-button" disabled={busy || !enabled || !valid}>{busy ? "Creating…" : "Create project"}</Button>
              </footer>
            </form>
    </Dialog>
  );
}
