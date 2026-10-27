import { useSource } from "@swarmie/client-ui";
import { Button, Modal } from "@swarmie/ui-web";
import { useState } from "react";

import { useProjects } from "./context";

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
  const { api } = useProjects();
  const state = useSource(api.state);
  const [name, setName] = useState(initialName);
  const [description, setDescription] = useState(initialDescription);
  const [node, setNode] = useState(nodes[0]?.id ?? "local");
  const [bindings, setBindings] = useState(
    state.personalFolders.filter((entry) => entry.projectId === projectId),
  );
  const [saving, setSaving] = useState(false);
  const submitLabel = projectId === null ? "Create project" : "Save changes";
  const paths = bindings.find((entry) => entry.nodeId === node)?.paths.join("\n") ?? "";
  async function save() {
    setSaving(true);
    try {
      const project =
        projectId === null ? await api.create({ name, description }) : { id: projectId };
      if (projectId !== null) {
        await api.update(projectId, { name, description });
      }
      await Promise.all(
        bindings.map(async (entry) => {
          await api.setPersonalFolders(project.id, entry);
        }),
      );
      api.closeDialog();
      if (projectId === null) {
        onCreated(project.id);
      }
    } catch (error: unknown) {
      api.fail(error);
    } finally {
      setSaving(false);
    }
  }
  return (
    <Modal
      isOpen
      onOpenChange={(isOpen) => {
        if (!isOpen && !saving) {
          api.closeDialog();
        }
      }}
    >
      <Modal.Backdrop>
        <Modal.Container>
          <Modal.Dialog
            aria-label={projectId === null ? "Create project" : "Edit project"}
            className="w-[calc(100vw-32px)] max-w-lg"
          >
            <Modal.Header>
              <Modal.Heading>
                {projectId === null ? "Create project" : "Edit project"}
              </Modal.Heading>
            </Modal.Header>
            <form
              onSubmit={(event) => {
                event.preventDefault();
                save().catch(api.fail);
              }}
              className="grid gap-4"
            >
              <label className="grid gap-2 text-xs">
                Project name
                <input
                  autoFocus
                  required
                  value={name}
                  onChange={(event) => {
                    setName(event.target.value);
                  }}
                  className="h-11 rounded-lg border border-border bg-default px-3 text-base md:text-sm"
                />
              </label>
              <label className="grid gap-2 text-xs">
                Description
                <textarea
                  rows={2}
                  value={description}
                  onChange={(event) => {
                    setDescription(event.target.value);
                  }}
                  className="resize-none rounded-lg border border-border bg-default px-3 py-2 text-base md:text-sm"
                />
              </label>
              <div className="border-t border-border pt-4">
                <h3 className="text-sm font-medium">Your folders</h3>
                <p className="mt-1 text-xs leading-5 text-muted">
                  These links are private to your profile. Adding a link does not grant filesystem
                  access.
                </p>
                <label className="mt-3 grid gap-2 text-xs">
                  Node
                  <select
                    value={node}
                    onChange={(event) => {
                      setNode(event.target.value);
                    }}
                    className="h-10 rounded-lg border border-border bg-default px-3 text-base md:text-sm"
                  >
                    {nodes.map((entry) => (
                      <option key={entry.id} value={entry.id}>
                        {entry.label}
                      </option>
                    ))}
                  </select>
                </label>
                <label className="mt-3 grid gap-2 text-xs">
                  Folder paths
                  <textarea
                    rows={2}
                    placeholder="One folder path per line"
                    value={paths}
                    onChange={(event) => {
                      const { value } = event.target;
                      setBindings((current) => [
                        ...current.filter((entry) => entry.nodeId !== node),
                        { projectId: projectId ?? "", nodeId: node, paths: value.split("\n") },
                      ]);
                    }}
                    className="resize-none rounded-lg border border-border bg-default px-3 py-2 text-base md:text-sm"
                  />
                </label>
              </div>
              {state.error === null ? null : (
                <p role="alert" className="text-xs text-danger">
                  {state.error}
                </p>
              )}
              <Modal.Footer>
                <Button variant="tertiary" isDisabled={saving} onPress={api.closeDialog}>
                  Cancel
                </Button>
                <Button
                  tone="brand"
                  shape="panel"
                  type="submit"
                  isDisabled={saving || !name.trim()}
                >
                  {saving ? "Saving…" : submitLabel}
                </Button>
              </Modal.Footer>
            </form>
          </Modal.Dialog>
        </Modal.Container>
      </Modal.Backdrop>
    </Modal>
  );
}
