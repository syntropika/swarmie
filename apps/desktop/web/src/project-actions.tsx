import { useState, useId } from "react";

import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Disclosure } from "@/components/ui/disclosure";
import { Input } from "@/components/ui/input";
import { Select } from "@/components/ui/select";

import { startAction } from "./action-boundary";
import { extensionResult } from "./components/extension-result";
import type { Thread } from "./protocol";
import { hasText } from "./value-presence";
import type { WorkspaceStore } from "./workspace";

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
        variant="ghost"
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
            <Disclosure.Summary>Publish repository</Disclosure.Summary>
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
                id={`${fieldPrefix}-field-2`}
                label="Repository visibility"
                value={visibility}
                onChange={setVisibility}
                options={[
                  { id: "private", label: "Private" },
                  { id: "public", label: "Public" },
                ]}
              />
            </label>
            <Button
              variant="ghost"
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
          </Disclosure>
          <Disclosure>
            <Disclosure.Summary>Worktree setup and cleanup</Disclosure.Summary>
            <Checkbox
              label="Initialize submodules during setup"
              checked={submodules}
              onChange={setSubmodules}
            />
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
                variant="ghost"
                size="sm"
                disabled={busy}
                onClick={() => {
                  startAction(
                    run(async () => {
                      await store.extension("worktree.setup.configure", {
                        projectId: project.id,
                        submodules,
                        commands: program ? [{ program, args: setupArguments(argumentsJson) }] : [],
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
                variant="ghost"
                size="sm"
                disabled={busy || project.worktrees.find((w) => w.id === thread.worktreeId)?.isMain}
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
                variant="ghost"
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
                              .map((r) => `${r.worktreeId}: ${r.removed ? "removed" : r.message}`)
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
          </Disclosure>
        </>
      )}
      {receipt && <output className="panel-note">{receipt}</output>}
    </>
  );
}
