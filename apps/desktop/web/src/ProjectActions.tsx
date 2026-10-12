import { Disclosure } from "@/components/ui/disclosure";
import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Select } from "@/components/ui/select";
import { Checkbox } from "@/components/ui/checkbox";
import type { Thread } from "./protocol";
import type { WorkspaceStore } from "./workspace";
export function ProjectActions({
  store,
  thread,
}: {
  store: WorkspaceStore;
  thread: Thread;
}) {
  const [visibility, setVisibility] = useState("private");
  const [publishName, setPublishName] = useState("");
  const [busy, setBusy] = useState(false);
  const [receipt, setReceipt] = useState("");
  const [program, setProgram] = useState("");
  const [argumentsJson, setArgumentsJson] = useState("[]");
  const [submodules, setSubmodules] = useState(false);
  const project = store
    .getSnapshot()
    .projects.find((p) => p.id === thread.projectId);
  const run = async (action: () => Promise<unknown>) => {
    setBusy(true);
    try {
      await action();
    } catch (e) {
      store.setError(e);
    } finally {
      setBusy(false);
    }
  };
  return (
    <>
      <Button
        variant="ghost"
        className="small-button"
        disabled={busy}
        onClick={() =>
          void run(async () => {
            const scratch = await store.invokeExtension<{ path: string }>(
              "workspace",
              "project.scratch",
              {},
            );
            await store.openProject(scratch.path);
          })
        }
      >
        Start a scratch conversation
      </Button>
      {project?.git && (
        <>
          <Disclosure>
            <Disclosure.Summary>Publish repository</Disclosure.Summary>
            <label>
              Repository name
              <Input
                value={publishName}
                onChange={(e) => setPublishName(e.target.value)}
                placeholder={project.name}
              />
            </label>
            <label>
              Visibility
              <Select label="Repository visibility" value={visibility} onChange={setVisibility} options={[{ id: "private", label: "Private" }, { id: "public", label: "Public" }]} />
            </label>
            <Button
              variant="ghost"
              className="small-button"
              disabled={busy}
              onClick={() =>
                void run(async () => {
                  await store.openWorkspace(thread);
                  const published = await store.invokeExtension<{
                    url: string;
                  }>("workspace", "project.publish", {
                    projectId: thread.worktreeId,
        ...(thread.sessionId ? { sessionId: thread.sessionId } : {}),
                    name: publishName || project.name,
                    visibility,
                    push: true,
                  });
                  setReceipt(`Repository published: ${published.url}`);
                })
              }
            >
              Create remote and push repository
            </Button>
          </Disclosure>
          <Disclosure>
            <Disclosure.Summary>Worktree setup and cleanup</Disclosure.Summary>
            <Checkbox label="Initialize submodules during setup" checked={submodules} onChange={setSubmodules} />
            <label>
              Setup executable
              <Input
                value={program}
                onChange={(e) => setProgram(e.target.value)}
                placeholder="Optional command, such as npm"
              />
            </label>
            <label>
              Arguments (JSON array)
              <Input
                value={argumentsJson}
                onChange={(e) => setArgumentsJson(e.target.value)}
                placeholder='["install"]'
              />
            </label>
            <div className="permission-actions">
              <Button
                variant="ghost"
                className="small-button"
                disabled={busy}
                onClick={() =>
                  void run(async () => {
                    await store.extension("worktree.setup.configure", {
                      projectId: project.id,
                      submodules,
                      commands: program
                        ? [{ program, args: JSON.parse(argumentsJson) }]
                        : [],
                    });
                    setReceipt("Worktree setup configuration saved.");
                  })
                }
              >
                Save setup defaults
              </Button>
              <Button
                variant="ghost"
                className="small-button"
                disabled={
                  busy ||
                  project.worktrees.find((w) => w.id === thread.worktreeId)
                    ?.isMain
                }
                onClick={() =>
                  void run(async () => {
                    await store.extension("worktree.setup.run", {
                      projectId: project.id,
                      worktreeId: thread.worktreeId,
                    });
                    setReceipt("Worktree setup completed.");
                  })
                }
              >
                Run setup in this worktree
              </Button>
              <Button
                variant="ghost"
                className="small-button"
                disabled={busy}
                onClick={() =>
                  void run(async () => {
                    const threads = store.getSnapshot().threads;
                    const worktreeIds = project.worktrees
                      .filter(
                        (w) =>
                          !w.isMain &&
                          !threads.some(
                            (t) =>
                              t.worktreeId === w.id &&
                              !!t.sessionId &&
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
                    }>("worktree.cleanup", {
                      projectId: project.id,
                      worktreeIds,
                    });
                    const removed = new Set(
                      result.results
                        .filter((r) => r.removed)
                        .map((r) => r.worktreeId),
                    );
                    await store.refreshProject(project.id);
                    setReceipt(
                      result.results.length
                        ? result.results
                            .map(
                              (r) =>
                                `${r.worktreeId}: ${r.removed ? "removed" : r.message}`,
                            )
                            .join("\n")
                        : "No unused managed worktrees to remove.",
                    );
                    if (removed.has(thread.worktreeId))
                      store.createThread(project.id);
                  })
                }
              >
                Remove unused clean worktrees
              </Button>
            </div>
          </Disclosure>
        </>
      )}
      {receipt && (
        <p className="panel-note" role="status">
          {receipt}
        </p>
      )}
    </>
  );
}
