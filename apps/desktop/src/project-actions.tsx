import { Button, Checkbox, Disclosure, Input, Select, ListBox } from "@swarmie/ui-web";
import { isValidElement, useState, useId } from "react";

import { startAction } from "~/src/action-boundary";
import { extensionResult } from "~/src/components/extension-result";
import type { Thread } from "~/src/protocol";
import { hasText } from "~/src/value-presence";
import type { WorkspaceStore } from "~/src/workspace";

function setupArguments(text: string): string[] {
  const value: unknown = JSON.parse(text);
  if (!Array.isArray(value)) {
    throw new TypeError("Setup arguments must be a JSON array of strings.");
  }
  return value.map((argument: unknown) => {
    if (typeof argument !== "string") {
      throw new TypeError("Each setup argument must be a string.");
    }
    return argument;
  });
}
export function ProjectActions({ store, thread }: { store: WorkspaceStore; thread: Thread }) {
  const fieldPrefix = useId();

  const [visibility, setVisibility] = useState("private");
  const [publishName, setPublishName] = useState("");
  const [busy, setBusy] = useState(false);
  const [receipt, setReceipt] = useState("");
  const [program, setProgram] = useState("");
  const [argumentsJson, setArgumentsJson] = useState("[]");
  const [submodules, setSubmodules] = useState(false);
  const project = store.getSnapshot().projects.find((p) => p.id === thread.projectId);
  const run = async (action: () => Promise<unknown>) => {
    setBusy(true);
    try {
      await action();
    } catch (error) {
      store.setError(error);
    } finally {
      setBusy(false);
    }
  };
  return (
    <>
      <Button
        variant="tertiary"
        size="sm"
        disabled={busy}
        onClick={() => {
          startAction(
            run(async () => {
              await store.createProject("scratch");
            }),
            store.setError.bind(store),
          );
        }}
      >
        Start a scratch conversation
      </Button>
      {project?.git === true && (
        <>
          <Disclosure>
            <Disclosure.Heading>
              <Button slot="trigger" variant="tertiary">
                <span>Publish repository</span>
                <Disclosure.Indicator />
              </Button>
            </Disclosure.Heading>
            <Disclosure.Content>
              <Disclosure.Body>
                <label htmlFor={`${fieldPrefix}-field-1`}>
                  Repository name
                  <Input
                    id={`${fieldPrefix}-field-1`}
                    value={publishName}
                    onChange={(e) => {
                      setPublishName(e.target.value);
                    }}
                    placeholder={project.name}
                  />
                </label>
                <label htmlFor={`${fieldPrefix}-field-2`}>
                  Visibility
                  <Select
                    aria-label="Repository visibility"
                    value={visibility}
                    onChange={(key) => {
                      if (typeof key === "string") {
                        setVisibility(key);
                      }
                    }}
                    placeholder="Select…"
                  >
                    <Select.Trigger id={`${fieldPrefix}-field-2`}>
                      <Select.Value />
                      <Select.Indicator />
                    </Select.Trigger>
                    <Select.Popover>
                      <ListBox aria-label="Repository visibility">
                        {[
                          { id: "private", label: "Private" },
                          { id: "public", label: "Public" },
                        ].map((option) => (
                          <ListBox.Item
                            key={option.id}
                            id={option.id}
                            textValue={option.label}
                            isDisabled={"disabled" in option && option.disabled === true}
                          >
                            {"icon" in option && isValidElement(option.icon) ? option.icon : null}
                            {option.label}
                            <ListBox.ItemIndicator />
                          </ListBox.Item>
                        ))}
                      </ListBox>
                    </Select.Popover>
                  </Select>
                </label>
                <Button
                  variant="tertiary"
                  size="sm"
                  disabled={busy}
                  onClick={() => {
                    startAction(
                      run(async () => {
                        const published = await store.invokeExtension<{
                          url: string;
                        }>(
                          "workspace",
                          "project.publish",
                          {
                            projectId: thread.worktreeId,
                            ...(hasText(thread.sessionId) ? { sessionId: thread.sessionId } : {}),
                            name: publishName || project.name,
                            visibility,
                            push: true,
                          },
                          extensionResult<{
                            url: string;
                          }>({
                            type: "object",
                            properties: { url: { type: "string" } },
                            required: ["url"],
                          }),
                        );
                        setReceipt(`Repository published: ${published.url}`);
                      }),
                      store.setError.bind(store),
                    );
                  }}
                >
                  Create remote and push repository
                </Button>
              </Disclosure.Body>
            </Disclosure.Content>
          </Disclosure>
          <Disclosure>
            <Disclosure.Heading>
              <Button slot="trigger" variant="tertiary">
                <span>Worktree setup and cleanup</span>
                <Disclosure.Indicator />
              </Button>
            </Disclosure.Heading>
            <Disclosure.Content>
              <Disclosure.Body>
                <Checkbox
                  isSelected={submodules}
                  onChange={setSubmodules}
                  aria-label="Initialize submodules during setup"
                >
                  <Checkbox.Content>
                    <Checkbox.Control>
                      <Checkbox.Indicator />
                    </Checkbox.Control>
                    Initialize submodules during setup
                  </Checkbox.Content>
                </Checkbox>
                <label htmlFor={`${fieldPrefix}-field-3`}>
                  Setup executable
                  <Input
                    id={`${fieldPrefix}-field-3`}
                    value={program}
                    onChange={(e) => {
                      setProgram(e.target.value);
                    }}
                    placeholder="Optional command, such as npm"
                  />
                </label>
                <label htmlFor={`${fieldPrefix}-field-4`}>
                  Arguments (JSON array)
                  <Input
                    id={`${fieldPrefix}-field-4`}
                    value={argumentsJson}
                    onChange={(e) => {
                      setArgumentsJson(e.target.value);
                    }}
                    placeholder='["install"]'
                  />
                </label>
                <div className="permission-actions">
                  <Button
                    variant="tertiary"
                    size="sm"
                    disabled={busy}
                    onClick={() => {
                      startAction(
                        run(async () => {
                          await store.extension("worktree.setup.configure", {
                            projectId: project.id,
                            submodules,
                            commands: program
                              ? [{ program, args: setupArguments(argumentsJson) }]
                              : [],
                          });
                          setReceipt("Worktree setup configuration saved.");
                        }),
                        store.setError.bind(store),
                      );
                    }}
                  >
                    Save setup defaults
                  </Button>
                  <Button
                    variant="tertiary"
                    size="sm"
                    disabled={
                      busy || project.worktrees.find((w) => w.id === thread.worktreeId)?.isMain
                    }
                    onClick={() => {
                      startAction(
                        run(async () => {
                          await store.extension("worktree.setup.run", {
                            projectId: project.id,
                            worktreeId: thread.worktreeId,
                          });
                          setReceipt("Worktree setup completed.");
                        }),
                        store.setError.bind(store),
                      );
                    }}
                  >
                    Run setup in this worktree
                  </Button>
                  <Button
                    variant="tertiary"
                    size="sm"
                    disabled={busy}
                    onClick={() => {
                      startAction(
                        run(async () => {
                          const { threads } = store.getSnapshot();
                          const worktreeIds = project.worktrees
                            .filter(
                              (w) =>
                                !w.isMain &&
                                !threads.some(
                                  (t) =>
                                    t.worktreeId === w.id &&
                                    hasText(t.sessionId) &&
                                    !["closed", "failed"].includes(t.state),
                                ),
                            )
                            .map((w) => w.id);
                          const result = await store.extension<{
                            results: {
                              worktreeId: string;
                              removed: boolean;
                              message?: string;
                            }[];
                          }>(
                            "worktree.cleanup",
                            {
                              projectId: project.id,
                              worktreeIds,
                            },
                            extensionResult<{
                              results: {
                                worktreeId: string;
                                removed: boolean;
                                message?: string;
                              }[];
                            }>({
                              type: "object",
                              properties: {
                                results: {
                                  type: "array",
                                  items: {
                                    type: "object",
                                    properties: {
                                      worktreeId: { type: "string" },
                                      removed: { type: "boolean" },
                                      message: { type: "string" },
                                    },
                                    required: ["worktreeId", "removed"],
                                  },
                                },
                              },
                              required: ["results"],
                            }),
                          );
                          const removed = new Set(
                            result.results.filter((r) => r.removed).map((r) => r.worktreeId),
                          );
                          await store.refreshProject(project.id);
                          setReceipt(
                            result.results.length
                              ? result.results
                                  .map(
                                    (r) => `${r.worktreeId}: ${r.removed ? "removed" : r.message}`,
                                  )
                                  .join("\n")
                              : "No unused managed worktrees to remove.",
                          );
                          if (removed.has(thread.worktreeId)) {
                            store.createThread(project.id);
                          }
                        }),
                        store.setError.bind(store),
                      );
                    }}
                  >
                    Remove unused clean worktrees
                  </Button>
                </div>
              </Disclosure.Body>
            </Disclosure.Content>
          </Disclosure>
        </>
      )}
      {receipt && <output className="panel-note">{receipt}</output>}
    </>
  );
}
