import { ChevronDown, Folder, FolderPlus, X } from "lucide-react";
import { useState, useId } from "react";

import { startAction, errorMessage, errorReporter } from "~/web/src/action-boundary";
import { Button } from "~/web/src/components/ui/button";
import { Checkbox } from "~/web/src/components/ui/checkbox";
import { Dialog } from "~/web/src/components/ui/dialog";
import { Dropdown } from "~/web/src/components/ui/dropdown";
import { Input } from "~/web/src/components/ui/input";
import { InputGroup } from "~/web/src/components/ui/input-group";
import { nonEmptyText } from "~/web/src/value-presence";
import type { WorkspaceStore } from "~/web/src/workspace";

type Source = "existing" | "create" | "clone";

export function ProjectDialog({ store, onClose }: { store: WorkspaceStore; onClose: () => void }) {
  const fieldId = useId();
  const [name, setName] = useState("");
  const [folder, setFolder] = useState("");
  const [draftPath, setDraftPath] = useState("");
  const [source, setSource] = useState<Source>("existing");
  const [editing, setEditing] = useState(false);
  const [url, setUrl] = useState("");
  const [git, setGit] = useState(true);
  const [busy, setBusy] = useState(false);
  const [issue, setIssue] = useState("");
  const state = store.getSnapshot();
  const workspace = state.extensions.some((extension) => extension.id === "workspace");
  const enabled = state.connected && (state.git || workspace);
  const valid =
    name.trim().length > 0 &&
    (source === "existing" ? folder.length > 0 : draftPath.trim().length > 0) &&
    (source !== "clone" || url.trim().length > 0);

  const addFolder = () => {
    const path = draftPath.trim();
    if (!path) {
      return;
    }
    setFolder(path);
    if (!name.trim()) {
      setName(folderName(path) || path);
    }
    setEditing(false);
    setIssue("");
  };
  const chooseSource = (next: Source) => {
    setSource(next);
    setEditing(true);
    setIssue("");
  };
  const submit = async () => {
    if (!valid || !enabled || busy) {
      return;
    }
    setBusy(true);
    setIssue("");
    try {
      const project =
        source === "existing"
          ? await store.openProject(folder, name.trim())
          : await store.createProject(
              source,
              { parent: draftPath.trim(), name: name.trim(), url: url.trim(), initializeGit: git },
              name.trim(),
            );
      if (project) {
        onClose();
      } else {
        setIssue(
          nonEmptyText(store.getSnapshot().error) ??
            "The folder could not be opened. Check its path and try again.",
        );
      }
    } catch (error) {
      setIssue(errorMessage(error));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog label="Create project" onClose={onClose} busy={busy} className="max-w-[522px]">
      <div className="project-dialog">
        <form
          onSubmit={(event) => {
            event.preventDefault();
            startAction(submit(), errorReporter(setIssue));
          }}
          aria-busy={busy}
        >
          <header className="project-dialog-header">
            <h2 className="text-xl font-semibold">Create project</h2>
            <Button
              variant="ghost"
              size="icon"
              aria-label="Close project dialog"
              disabled={busy}
              onClick={onClose}
            >
              <X size={15} />
            </Button>
          </header>
          <div className="project-dialog-body">
            <div className="project-name-field">
              <InputGroup
                icon={<Folder size={15} />}
                autoFocus
                aria-label="Project name"
                placeholder="Project name"
                value={name}
                disabled={busy}
                onChange={(event) => {
                  setName(event.target.value);
                }}
              />
            </div>
            <p className="project-source-label" id={`${fieldId}-source`}>
              Source folders
            </p>
            <fieldset className="project-source-box" aria-labelledby={`${fieldId}-source`}>
              <Dropdown>
                <Button variant="ghost" className="max-w-full" disabled={busy}>
                  {(() => {
                    if (source === "existing") {
                      return "Add a folder on this computer";
                    }
                    if (source === "create") {
                      return "Create a new folder";
                    }
                    return "Clone a repository";
                  })()}
                  <ChevronDown size={13} />
                </Button>
                <Dropdown.Popover className="min-w-44">
                  <Dropdown.Menu aria-label="Project source">
                    <Dropdown.Item
                      id="existing"
                      onAction={() => {
                        chooseSource("existing");
                      }}
                    >
                      Add an existing folder
                    </Dropdown.Item>
                    <Dropdown.Item
                      id="create"
                      isDisabled={!workspace}
                      onAction={() => {
                        chooseSource("create");
                      }}
                    >
                      Create a new folder
                    </Dropdown.Item>
                    <Dropdown.Item
                      id="clone"
                      isDisabled={!workspace}
                      onAction={() => {
                        chooseSource("clone");
                      }}
                    >
                      Clone a repository
                    </Dropdown.Item>
                  </Dropdown.Menu>
                </Dropdown.Popover>
              </Dropdown>
              {(() => {
                if (source === "existing" && folder && !editing) {
                  return (
                    <div className="project-source-folder">
                      <Folder size={17} aria-hidden="true" />
                      <span title={folder}>
                        {folderName(folder)}
                        <small>{folder}</small>
                      </span>
                      <Button
                        variant="ghost"
                        size="icon"
                        aria-label="Remove source folder"
                        disabled={busy}
                        onClick={() => {
                          setFolder("");
                          setDraftPath("");
                        }}
                      >
                        <X size={14} />
                      </Button>
                    </div>
                  );
                }
                if (editing) {
                  return (
                    <div className="project-source-fields">
                      <label htmlFor={`${fieldId}-path`}>
                        {source === "existing" ? "Folder path" : "Parent folder"}
                        <Input
                          id={`${fieldId}-path`}
                          autoFocus
                          aria-label={source === "existing" ? "Folder path" : "Parent folder"}
                          placeholder="Absolute directory path"
                          value={draftPath}
                          disabled={busy}
                          onChange={(event) => {
                            setDraftPath(event.target.value);
                          }}
                          onKeyDown={(event) => {
                            if (event.key === "Enter" && source === "existing") {
                              event.preventDefault();
                              addFolder();
                            }
                          }}
                        />
                      </label>
                      {source === "existing" && (
                        <Button
                          variant="secondary"
                          disabled={busy || !draftPath.trim()}
                          onClick={addFolder}
                        >
                          Add folder
                        </Button>
                      )}
                      {source === "create" && (
                        <Checkbox
                          label="Initialize a Git repository"
                          checked={git}
                          disabled={busy}
                          onChange={setGit}
                        />
                      )}
                      {source === "clone" && (
                        <label htmlFor={`${fieldId}-url`}>
                          Repository URL
                          <Input
                            id={`${fieldId}-url`}
                            aria-label="Repository URL"
                            placeholder="Repository URL"
                            value={url}
                            disabled={busy}
                            onChange={(event) => {
                              setUrl(event.target.value);
                            }}
                          />
                        </label>
                      )}
                    </div>
                  );
                }
                return (
                  <Button
                    variant="secondary"
                    disabled={busy || !enabled}
                    onClick={() => {
                      setEditing(true);
                    }}
                  >
                    <FolderPlus size={14} />
                    Add
                  </Button>
                );
              })()}
            </fieldset>
            {!enabled && (
              <p className="project-dialog-error" role="alert">
                Connect to a daemon with the Workspace or Git extension to add a project.
              </p>
            )}
            {issue && (
              <p className="project-dialog-error" role="alert">
                {issue}
              </p>
            )}
          </div>
          <footer className="project-dialog-footer">
            <Button variant="ghost" disabled={busy} onClick={onClose}>
              Cancel
            </Button>
            <Button type="submit" className="min-w-32" disabled={busy || !enabled || !valid}>
              {busy ? "Creating…" : "Create project"}
            </Button>
          </footer>
        </form>
      </div>
    </Dialog>
  );
}

function folderName(path: string) {
  let name = "";
  for (const part of path.split(/[\\/]/u)) {
    if (part.length > 0) {
      name = part;
    }
  }
  return name;
}
