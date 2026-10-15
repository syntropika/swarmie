import { X, RefreshCw, Check, Plus, Trash2 } from "lucide-react";
// Copyright 2026 Syntropika
// SPDX-License-Identifier: Apache-2.0
import { useId, lazy, Suspense } from "react";

import { startAction } from "~/src/action-boundary";
import { Button } from "~/src/components/ui/button";
import { Input } from "~/src/components/ui/input";
import { ResizeHandle } from "~/src/components/ui/resize-handle";
import { Select } from "~/src/components/ui/select";
import { ContributionSurface, ContributionSlot } from "~/src/contribution-host";
import { Conversation, PatchCopy } from "~/src/conversation";
import { ProjectActions } from "~/src/project-actions";
import { RequestCard } from "~/src/request-card";
import { SessionControls } from "~/src/session-controls";
import { hasText } from "~/src/value-presence";
import { IconButton } from "~/src/workspace-icon-button";
import type { WorkspaceViewState } from "~/src/workspace-state";

const ExtensionPanel = lazy(
  async () =>
    await import("~/src/extension-panel").then((module) => ({
      default: module.ExtensionPanel,
    })),
);

export function WorkspaceConversation({
  view,
}: {
  view: Pick<
    WorkspaceViewState,
    | "store"
    | "state"
    | "extensionScreen"
    | "surfaceContext"
    | "closeScreen"
    | "undo"
    | "setUndo"
    | "active"
    | "project"
    | "ready"
    | "composerContext"
    | "composerControls"
  >;
}) {
  const {
    store,
    state,
    extensionScreen,
    surfaceContext,
    closeScreen,
    undo,
    setUndo,
    active,
    project,
    ready,
    composerContext,
    composerControls,
  } = view;
  return (
    <main className="main-content">
      {undo && (
        <output className="undo-banner">
          <span>{undo.title}</span>
          <Button
            variant="ghost"
            size="xs"
            onClick={() => {
              startAction(Promise.resolve(undo.run()), store.setError.bind(store));
              setUndo(undefined);
            }}
          >
            Undo
          </Button>
          <Button
            variant="ghost"

            aria-label="Dismiss undo"
            onClick={() => {
              setUndo(undefined);
            }}
          >
            <X size={12} />
          </Button>
        </output>
      )}
      {state.error && (
        <div className="error-banner" role="alert">
          <span>{state.error}</span>
          <IconButton
            label="Dismiss error"
            onClick={() => {
              store.dismissError();
            }}
          >
            <X size={14} />
          </IconButton>
        </div>
      )}
      {active.permissions.map((p) => (
        <RequestCard
          key={p.id}
          permission={p}
          onAnswer={async (option, data) => {
            await store.respond(p.id, option, data);
          }}
        />
      ))}
      {hasText(extensionScreen) ? (
        <ContributionSurface
          kind="screen"
          id={extensionScreen}
          context={surfaceContext}
          onClose={closeScreen}
        />
      ) : (
        <Conversation
          key={active.id}
          thread={active}
          store={store}
          controls={composerControls}
          permissions={
            <SessionControls
              kind="settings"
              thread={active}
              store={store}
              connected={state.connected}
            />
          }
          context={composerContext}
          projectName={project?.name}
          ready={ready}
        />
      )}
    </main>
  );
}

export function WorkspaceDetailPanel({
  view,
}: {
  view: Pick<
    WorkspaceViewState,
    | "store"
    | "state"
    | "panel"
    | "setPanel"
    | "surfaceContext"
    | "extensionPanel"
    | "setExtensionPanel"
    | "branch"
    | "setBranch"
    | "working"
    | "active"
    | "project"
    | "tree"
    | "sessionOpen"
    | "perform"
  >;
}) {
  const {
    store,
    state,
    panel,
    setPanel,
    surfaceContext,
    extensionPanel,
    setExtensionPanel,
    working,
    active,
    perform,
  } = view;
  return (
    <aside
      className="detail-panel"
      aria-label={(() => {
        if (panel === "review") {
          return "Change review";
        }
        if (panel === "project") {
          return "Project settings";
        }
        if (panel === "extensions") {
          return "Workspace tools";
        }
        return "Connection settings";
      })()}
    >
      <ResizeHandle
        variant="panel"

        aria-label="Resize workspace panel"
        aria-valuemin={300}
        aria-valuemax={800}
        aria-valuenow={active.layout?.width ?? 370}
        aria-orientation="vertical"
        onKeyDown={(e) => {
          if (["ArrowLeft", "ArrowRight"].includes(e.key)) {
            e.preventDefault();
            store.updateThread(active.id, {
              layout: {
                ...active.layout,
                width: Math.max(
                  300,
                  Math.min(800, (active.layout?.width ?? 370) + (e.key === "ArrowLeft" ? 20 : -20)),
                ),
                panel,
              },
            });
          }
        }}
        onPointerDown={(e) => {
          e.currentTarget.setPointerCapture(e.pointerId);
        }}
        onPointerMove={(e) => {
          if (e.currentTarget.hasPointerCapture(e.pointerId)) {
            store.updateThread(active.id, {
              layout: {
                ...active.layout,
                width: Math.max(300, Math.min(800, window.innerWidth - e.clientX)),
                panel,
              },
            });
          }
        }}
        onPointerUp={(e) => {
          e.currentTarget.releasePointerCapture(e.pointerId);
        }}
      />
      {panel !== "client" && (
        <div className="detail-heading">
          <h2>
            {(() => {
              if (panel === "review") {
                return "Changes";
              }
              if (panel === "project") {
                return "Project";
              }
              if (panel === "extensions") {
                return "Workspace tools";
              }
              return "Settings";
            })()}
          </h2>
          <IconButton
            label="Close panel"
            onClick={() => {
              setPanel(null);
            }}
          >
            <X size={17} />
          </IconButton>
        </div>
      )}
      {panel !== "client" && <ContributionSlot name="panel.toolbar" />}
      {panel === "client" && hasText(extensionPanel) && (
        <ContributionSurface
          kind="panel"
          id={extensionPanel}
          context={surfaceContext}
          onClose={() => {
            setExtensionPanel(undefined);
            setPanel(null);
          }}
        />
      )}
      {panel === "extensions" && (
        <Suspense fallback={<p className="panel-note">Loading workspace tools…</p>}>
          <ExtensionPanel
            key={active.id}
            store={store}
            thread={active}
            extensions={state.extensions}
          />
        </Suspense>
      )}
      {panel === "project" && <WorkspaceProjectPanel view={view} />}
      {panel === "review" && (
        <div className="review-body">
          <div className="review-toolbar">
            <span>Compared with HEAD</span>
            <IconButton
              label="Refresh changes"
              disabled={working}
              onClick={() => {
                startAction(
                  perform(async () => {
                    await store.review();
                  }),
                  store.setError.bind(store),
                );
              }}
            >
              <RefreshCw size={14} />
            </IconButton>
          </div>
          {state.review ? (
            <>
              <div className="changed-files">
                {state.review.files.map((f) => (
                  <div key={f.path}>
                    <span className="file-status">{f.status.trim()}</span>
                    <span title={f.path}>{f.path}</span>
                  </div>
                ))}
              </div>
              {state.review.diff ? (
                <pre className="patch">
                  {reviewLines(state.review.diff).map(({ line, id }) => (
                    <div
                      key={id}
                      className={(() => {
                        if (line.startsWith("+") && !line.startsWith("+++")) {
                          return "addition";
                        }
                        if (line.startsWith("-") && !line.startsWith("---")) {
                          return "deletion";
                        }
                        if (line.startsWith("@@")) {
                          return "hunk";
                        }
                        return "";
                      })()}
                    >
                      {line || " "}
                    </div>
                  ))}
                </pre>
              ) : (
                <div className="review-empty">
                  <Check size={22} />
                  <p>Working tree is clean.</p>
                </div>
              )}
              {(state.review.truncated || state.review.filesTruncated) && (
                <p className="panel-note">
                  This review is truncated. Inspect the full changes with Git.
                </p>
              )}
              <div className="review-footer">
                <PatchCopy text={state.review.diff} />
              </div>
            </>
          ) : (
            <p className="panel-note">
              {working ? "Reading changes…" : "Open a project and refresh its changes."}
            </p>
          )}
        </div>
      )}
      {panel !== "client" && <ContributionSlot name="panel.status" />}
    </aside>
  );
}

function reviewLines(diff: string) {
  const occurrences = new Map<string, number>();
  return diff.split("\n").map((line) => {
    const occurrence = (occurrences.get(line) ?? 0) + 1;
    occurrences.set(line, occurrence);
    return { id: `${line}:${occurrence}`, line };
  });
}

function WorkspaceProjectPanel({
  view,
}: {
  view: Pick<
    WorkspaceViewState,
    | "store"
    | "state"
    | "branch"
    | "setBranch"
    | "working"
    | "active"
    | "project"
    | "tree"
    | "sessionOpen"
    | "perform"
  >;
}) {
  const { store, state, branch, setBranch, working, active, project, tree, sessionOpen, perform } =
    view;
  const worktreeControlId = useId();
  const branchId = useId();
  return (
    <div className="panel-body">
      {state.extensions.some((e) => e.id === "workspace") && (
        <ProjectActions store={store} thread={active} />
      )}
      {project && (
        <>
          <hr />
          <h3>{project.name}</h3>
          <p className="path-label" title={project.path}>
            {project.path}
          </p>
          <label htmlFor={worktreeControlId}>
            Worktree
            <Select
              label="Worktree"
              id={worktreeControlId}
              disabled={hasText(active.sessionId)}
              value={active.worktreeId}
              onChange={(worktreeId) => {
                store.updateThread(active.id, { worktreeId });
              }}
              options={project.worktrees
                .filter((w) => !w.prunable)
                .map((w) => ({
                  id: w.id,
                  label:
                    (w.branch ?? (project.git === true ? "Detached HEAD" : "Local folder")) +
                    (w.isMain ? " · main worktree" : ""),
                }))}
            />
          </label>
          <form
            onSubmit={(e) => {
              e.preventDefault();
              startAction(
                perform(async () => {
                  await store.createWorktree(branch);
                  setBranch("");
                }),
                store.setError.bind(store),
              );
            }}
          >
            <label htmlFor={branchId}>
              New branch
              <Input
                id={branchId}
                aria-label="New branch"
                value={branch}
                onChange={(e) => {
                  setBranch(e.target.value);
                }}
                placeholder="feature/my-task"
                disabled={hasText(active.sessionId)}
              />
            </label>
            <Button
              variant="ghost"
              size="xs"
              type="submit"
              disabled={
                !(project.git === true) || hasText(active.sessionId) || working || !branch.trim()
              }
            >
              <Plus size={14} />
              Create worktree
            </Button>
          </form>
          {sessionOpen ? (
            <Button
              variant="ghost"
              size="xs"
              disabled={working}
              onClick={() => {
                startAction(
                  perform(async () => {
                    await store.close(active.id);
                  }),
                  store.setError.bind(store),
                );
              }}
            >
              Close session
            </Button>
          ) : (
            tree &&
            !tree.isMain && (
              <Button
                variant="ghost"
                size="xs"
                disabled={working}
                onClick={() => {
                  startAction(
                    perform(async () => {
                      await store.removeWorktree();
                    }),
                    store.setError.bind(store),
                  );
                }}
              >
                <Trash2 size={13} />
                Remove clean worktree
              </Button>
            )
          )}
          <p className="panel-note">
            Start a new chat to choose a different worktree. Removal retains the branch. Git refuses
            worktrees with pending changes.
          </p>
        </>
      )}
    </div>
  );
}
