import { useSource } from "@swarmie/client-ui";
import {
  Button,
  InputGroup,
  Label,
  ListBox,
  ResponsiveDialog,
  Select,
  Surface,
  TextArea,
  TextField,
} from "@swarmie/ui-web";
import { Folder, FolderPlus, Monitor, X } from "lucide-react";
import { useState } from "react";

import { useProjects } from "./context";
import { FolderPicker } from "./folder-picker";
import { saveProjectChanges } from "./save";

export function ProjectDialog() {
  const { api, nodes, open } = useProjects();
  const state = useSource(api.state);
  const availableNodes = useSource(nodes);
  if (state.dialog === null) {
    return null;
  }
  const project = state.records.find((entry) => entry.id === state.dialog?.projectId);
  return (
    <ProjectEditor
      key={project?.id ?? "new"}
      projectId={project?.id ?? null}
      name={project?.name ?? ""}
      description={project?.description ?? ""}
      nodes={availableNodes}
      onCreated={open}
    />
  );
}
function ProjectEditor({
  projectId,
  name: initialName,
  description: initialDescription,
  nodes,
  onCreated,
}: {
  projectId: string | null;
  name: string;
  description: string;
  nodes: readonly { id: string; label: string }[];
  onCreated: (id: string) => void;
}) {
  const { api, directories } = useProjects();
  const state = useSource(api.state);
  const [name, setName] = useState(initialName);
  const [description, setDescription] = useState(initialDescription);
  const [node, setNode] = useState(nodes[0]?.id ?? "local");
  const [bindings, setBindings] = useState(
    state.personalFolders.filter((entry) => entry.projectId === projectId),
  );
  const [saving, setSaving] = useState(false);
  const [addingFolder, setAddingFolder] = useState(false);
  const editorTitle = projectId === null ? "Create project" : "Edit project";
  const dialogTitle = addingFolder ? "Choose a folder" : editorTitle;
  const submitLabel = projectId === null ? "Create project" : "Save changes";
  const paths = bindings.find((entry) => entry.nodeId === node)?.paths ?? [];
  function setPaths(next: string[]) {
    setBindings((current) => [
      ...current.filter((entry) => entry.nodeId !== node),
      { projectId: projectId ?? "", nodeId: node, paths: next },
    ]);
  }
  async function save() {
    const currentScope = api.scope();
    setSaving(true);
    try {
      await saveProjectChanges(
        api,
        { id: projectId, name, description, folders: bindings },
        onCreated,
      );
    } catch (error: unknown) {
      if (currentScope()) {
        api.fail(error);
      }
    } finally {
      setSaving(false);
    }
  }
  return (
    <ResponsiveDialog
      isOpen
      onOpenChange={(isOpen) => {
        if (!isOpen && !saving) {
          api.closeDialog();
        }
      }}
    >
      <ResponsiveDialog.Backdrop>
        <ResponsiveDialog.Container>
          <ResponsiveDialog.Dialog aria-label={dialogTitle}>
            <ResponsiveDialog.Header>
              <ResponsiveDialog.Heading>{dialogTitle}</ResponsiveDialog.Heading>
              <ResponsiveDialog.CloseTrigger isDisabled={saving} />
            </ResponsiveDialog.Header>
            {addingFolder ? (
              <FolderPicker
                key={node}
                browser={directories}
                nodeId={node}
                choose={(path) => {
                  if (!paths.includes(path)) {
                    setPaths([...paths, path]);
                  }
                  setAddingFolder(false);
                }}
                cancel={() => {
                  setAddingFolder(false);
                }}
              />
            ) : (
              <form
                onSubmit={(event) => {
                  event.preventDefault();
                  save().catch(api.fail);
                }}
                className="grid gap-4"
              >
                <TextField aria-label="Project name" value={name} onChange={setName} isRequired>
                  <InputGroup>
                    <InputGroup.Prefix>
                      <Folder aria-hidden="true" className="size-4" />
                    </InputGroup.Prefix>
                    <InputGroup.Input autoFocus placeholder="Project name" />
                  </InputGroup>
                </TextField>
                {projectId === null ? null : (
                  <TextField value={description} onChange={setDescription}>
                    <Label>Description</Label>
                    <TextArea rows={2} />
                  </TextField>
                )}
                <section aria-label="Source folders" className="grid gap-2">
                  <div className="flex flex-wrap items-center justify-between gap-3">
                    <h3 className="text-sm font-medium">Source folders</h3>
                    {nodes.length > 1 ? (
                      <Select
                        aria-label="Node"
                        value={node}
                        onChange={(value) => {
                          if (typeof value === "string") {
                            setNode(value);
                            setAddingFolder(false);
                          }
                        }}
                      >
                        <Select.Trigger>
                          <Select.Value />
                          <Select.Indicator />
                        </Select.Trigger>
                        <Select.Popover>
                          <ListBox>
                            {nodes.map((entry) => (
                              <ListBox.Item key={entry.id} id={entry.id} textValue={entry.label}>
                                {entry.label}
                                <ListBox.ItemIndicator />
                              </ListBox.Item>
                            ))}
                          </ListBox>
                        </Select.Popover>
                      </Select>
                    ) : (
                      <span className="flex items-center gap-2 text-xs text-muted">
                        <Monitor aria-hidden="true" className="size-4" />
                        {nodes[0]?.label ?? "This computer"}
                      </span>
                    )}
                  </div>
                  <Surface variant="secondary" className="overflow-hidden">
                    {paths.map((path) => (
                      <div key={path} className="flex items-center gap-2 px-3 py-2">
                        <Folder aria-hidden="true" className="size-4 shrink-0 text-muted" />
                        <span title={path} className="min-w-0 flex-1 truncate text-sm">
                          {path.split(/[\\/]/u).findLast((part) => part !== "") ?? path}
                        </span>
                        <Button
                          isIconOnly
                          size="sm"
                          variant="tertiary"
                          aria-label={`Remove ${path}`}
                          onPress={() => {
                            setPaths(paths.filter((entry) => entry !== path));
                          }}
                        >
                          <X />
                        </Button>
                      </div>
                    ))}
                    <div
                      className={
                        paths.length === 0 ? "flex min-h-24 items-center justify-center p-1" : "p-1"
                      }
                    >
                      <Button
                        variant="ghost"
                        className={paths.length === 0 ? "" : "w-full"}
                        onPress={() => {
                          setAddingFolder(true);
                        }}
                      >
                        <FolderPlus />
                        Add folder
                      </Button>
                    </div>
                  </Surface>
                </section>
                {state.error === null ? null : (
                  <p role="alert" className="text-xs text-danger">
                    {state.error}
                  </p>
                )}
                <ResponsiveDialog.Footer>
                  <Button variant="tertiary" isDisabled={saving} onPress={api.closeDialog}>
                    Cancel
                  </Button>
                  <Button type="submit" isDisabled={saving || !name.trim()}>
                    {saving ? "Saving…" : submitLabel}
                  </Button>
                </ResponsiveDialog.Footer>
              </form>
            )}
          </ResponsiveDialog.Dialog>
        </ResponsiveDialog.Container>
      </ResponsiveDialog.Backdrop>
    </ResponsiveDialog>
  );
}
