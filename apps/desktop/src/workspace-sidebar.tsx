import {
  ChevronDown,
  Check,
  Search,
  SquarePen,
  Plus,
  FolderOpen,
  MoreHorizontal,
} from "lucide-react";
// Copyright 2026 Syntropika
// SPDX-License-Identifier: Apache-2.0
import { useRef } from "react";

import { startAction } from "~/src/action-boundary";
import { Button } from "~/src/components/ui/button";
import { Dropdown } from "~/src/components/ui/dropdown";
import { Input } from "~/src/components/ui/input";
import { ResizeHandle } from "~/src/components/ui/resize-handle";
import { Tooltip, TooltipContent } from "~/src/components/ui/tooltip";
import { ContributionSlot } from "~/src/contribution-host";
import type { Thread } from "~/src/protocol";
import { SidebarChatActions } from "~/src/sidebar-chat-actions";
import { sortThreads } from "~/src/sidebar-preferences";
import { SidebarSection, SidebarSortMenu } from "~/src/sidebar-section";
import { hasText } from "~/src/value-presence";
import { IconButton } from "~/src/workspace-icon-button";
import type { WorkspaceViewState } from "~/src/workspace-state";

export function WorkspaceSidebar({
  view,
}: {
  view: Pick<
    WorkspaceViewState,
    | "nodes"
    | "state"
    | "sidebarWidth"
    | "setSidebarWidth"
    | "organization"
    | "setOrganization"
    | "now"
    | "showSnoozed"
    | "setShowSnoozed"
    | "archived"
    | "query"
    | "setQuery"
    | "search"
    | "setSearch"
    | "setProjectDialog"
    | "collapsed"
    | "sidebarSort"
    | "setSidebarSort"
    | "filtered"
    | "pinned"
    | "projects"
    | "newChat"
    | "toggleProject"
    | "store"
    | "setUndo"
    | "setRenaming"
    | "setTitle"
    | "active"
    | "perform"
    | "select"
  >;
}) {
  const {
    nodes,
    state,
    sidebarWidth,
    setSidebarWidth,
    organization,
    setOrganization,
    now,
    showSnoozed,
    setShowSnoozed,
    archived,
    query,
    setQuery,
    search,
    setSearch,
    setProjectDialog,
    collapsed,
    sidebarSort,
    setSidebarSort,
    filtered,
    pinned,
    projects,
    newChat,
    toggleProject,
  } = view;
  const threadRow = (thread: Thread) => <ThreadRow thread={thread} view={view} />;
  return (
    <aside className="sidebar" aria-label="Projects and chats">
      <ResizeHandle
        variant="sidebar"

        aria-label="Resize sidebar"
        aria-orientation="vertical"
        aria-valuemin={220}
        aria-valuemax={360}
        aria-valuenow={sidebarWidth}
        onKeyDown={(event) => {
          if (event.key === "ArrowLeft" || event.key === "ArrowRight") {
            event.preventDefault();
            setSidebarWidth((width) =>
              Math.max(220, Math.min(360, width + (event.key === "ArrowRight" ? 20 : -20))),
            );
          }
        }}
        onPointerDown={(event) => {
          event.currentTarget.setPointerCapture(event.pointerId);
        }}
        onPointerMove={(event) => {
          if (event.currentTarget.hasPointerCapture(event.pointerId)) {
            setSidebarWidth(Math.max(220, Math.min(360, event.clientX - 50)));
          }
        }}
        onPointerUp={(event) => {
          event.currentTarget.releasePointerCapture(event.pointerId);
        }}
      />
      <div className="sidebar-heading">
        <Dropdown>
          <Button variant="wordmark" aria-label="Workspace menu">
            swarmie
            <ChevronDown size={12} />
          </Button>
          <Dropdown.Popover>
            <Dropdown.Menu aria-label="Workspace organization">
              <Dropdown.Item
                id="projects"
                textValue="Group by project"
                onAction={() => {
                  setOrganization("projects");
                }}
              >
                Projects{organization === "projects" && <Check size={14} />}
              </Dropdown.Item>
              <Dropdown.Item
                id="time"
                textValue="Group by recent activity"
                onAction={() => {
                  setOrganization("time");
                }}
              >
                Recent activity{organization === "time" && <Check size={14} />}
              </Dropdown.Item>
              <Dropdown.Item
                id="working"
                textValue="Group by working status"
                onAction={() => {
                  setOrganization("working");
                }}
              >
                Working{organization === "working" && <Check size={14} />}
              </Dropdown.Item>
              <Dropdown.Item
                id="settled"
                textValue={showSnoozed ? "Hide settled conversations" : "Show all conversations"}
                onAction={() => {
                  setShowSnoozed((value) => !value);
                }}
              >
                {showSnoozed ? "Hide settled conversations" : "Show all conversations"}
              </Dropdown.Item>
            </Dropdown.Menu>
          </Dropdown.Popover>
        </Dropdown>
        <IconButton
          label="Search chats"
          active={search}
          onClick={() => {
            setSearch((value) => !value);
            if (search) {
              setQuery("");
            }
          }}
        >
          <Search size={16} />
        </IconButton>
      </div>
      <div className="sidebar-navigation">
        <Button
          variant="navigation"
          onClick={() => {
            newChat();
          }}
        >
          <SquarePen size={16} />
          <span>New chat</span>
        </Button>
      </div>
      {search && (
        <Input
          variant="search"
          autoFocus
          aria-label="Search chats"
          placeholder="Search chats…"
          value={query}
          onChange={(e) => {
            setQuery(e.target.value);
          }}
        />
      )}
      <div className="sidebar-scroll">
        {organization !== "projects" && (
          <section className="sidebar-section">
            {organization === "working" ? (
              <>
                <h2>Working</h2>
                {filtered
                  .filter((t) =>
                    ["starting", "running", "needs_input", "cancelling"].includes(t.state),
                  )
                  .map(threadRow)}
                <h2>Other conversations</h2>
                {filtered
                  .filter(
                    (t) => !["starting", "running", "needs_input", "cancelling"].includes(t.state),
                  )
                  .map(threadRow)}
              </>
            ) : (
              ["Today", "This week", "Earlier"].map((label, index) => (
                <div key={label}>
                  <h2>{label}</h2>
                  {filtered
                    .filter((t) => {
                      const age = now - (t.updatedAtMs ?? t.createdAtMs ?? 0);
                      return (() => {
                        if (index === 0) {
                          return age < 86_400_000;
                        }
                        if (index === 1) {
                          return age >= 86_400_000 && age < 7 * 86_400_000;
                        }
                        return age >= 7 * 86_400_000;
                      })();
                    })
                    .map(threadRow)}
                </div>
              ))
            )}
          </section>
        )}
        <div hidden={organization !== "projects"}>
          {pinned.length > 0 && (
            <SidebarSection
              label="Pinned"
              collapsed={collapsed.has("section:pinned")}
              onToggle={() => {
                toggleProject("section:pinned");
              }}
              actions={
                <SidebarSortMenu
                  label="Pinned"
                  value={sidebarSort.pinned}
                  onChange={(value) => {
                    setSidebarSort((previous) => ({ ...previous, pinned: value }));
                  }}
                />
              }
            >
              {pinned.map(threadRow)}
            </SidebarSection>
          )}
          <SidebarSection
            label={archived ? "Archived chats" : "Projects"}
            collapsed={collapsed.has("section:projects")}
            onToggle={() => {
              toggleProject("section:projects");
            }}
            actions={
              <>
                <SidebarSortMenu
                  label="Projects"
                  value={sidebarSort.projects}
                  onChange={(value) => {
                    setSidebarSort((previous) => ({ ...previous, projects: value }));
                  }}
                />
                {!archived && (
                  <Tooltip>
                    <Button
                      variant="ghost"
                      size="icon-sm"
                      aria-label="Add new project"
                      tone="muted"
                      onClick={() => {
                        setProjectDialog(true);
                      }}
                    >
                      <Plus size={14} />
                    </Button>
                    <TooltipContent>Add new project</TooltipContent>
                  </Tooltip>
                )}
              </>
            }
          >
            {projects.map((p) => (
              <div className="project-group group/project" key={p.id}>
                <div className="project-heading">
                  <Button
                    variant="ghost"
                    onClick={() => {
                      toggleProject(p.id);
                    }}
                  >
                    <span className="project-icon">
                      <FolderOpen size={14} />
                      <span />
                    </span>
                    <span className="project-name">{p.name}</span>
                    <ChevronDown
                      size={12}
                      className={`shrink-0 transition-transform ${collapsed.has(p.id) ? "-rotate-90" : "opacity-0 group-focus-within/project:opacity-100 group-hover/project:opacity-100"}`}
                    />
                  </Button>
                  <ContributionSlot
                    name="project.actions"
                    context={{
                      projectId: p.id,
                      worktreeId: undefined,
                      conversationId: undefined,
                      sessionId: undefined,
                      composerReady: false,
                    }}
                  />
                  <Dropdown>
                    <Button
                      variant="ghost"
                      size="icon-sm"
                      aria-label={`Project options: ${p.name}`}
                    >
                      <MoreHorizontal size={14} />
                    </Button>
                    <Dropdown.Popover>
                      <Dropdown.Menu aria-label={`Actions for project ${p.name}`}>
                        <Dropdown.Item
                          id="new"
                          onAction={() => {
                            newChat(p.id);
                          }}
                        >
                          New chat
                        </Dropdown.Item>
                        <ContributionSlot
                          name="project.contextMenu"
                          context={{
                            projectId: p.id,
                            worktreeId: undefined,
                            conversationId: undefined,
                            sessionId: undefined,
                            composerReady: false,
                          }}
                          menu
                        />
                      </Dropdown.Menu>
                    </Dropdown.Popover>
                  </Dropdown>
                  <div className="pointer-events-none opacity-0 group-focus-within/project:pointer-events-auto group-focus-within/project:opacity-100 group-hover/project:pointer-events-auto group-hover/project:opacity-100">
                    <Tooltip>
                      <Button
                        variant="ghost"
                        size="icon-sm"
                        aria-label={`New chat in ${p.name}`}
                        tone="muted"
                        onClick={() => {
                          newChat(p.id);
                        }}
                      >
                        <Plus size={14} />
                      </Button>
                      <TooltipContent>New chat</TooltipContent>
                    </Tooltip>
                  </div>
                </div>
                {!collapsed.has(p.id) && (
                  <div className="project-chats">
                    {sortThreads(
                      filtered.filter((t) => t.projectId === p.id && !t.pinned),
                      sidebarSort.recents,
                    ).map(threadRow)}
                    {!filtered.some((t) => t.projectId === p.id) && (
                      <p className="no-chats">No chats yet</p>
                    )}
                  </div>
                )}
              </div>
            ))}
          </SidebarSection>
          {filtered.some((t) => !t.projectId && !t.pinned) && (
            <SidebarSection
              label="Recents"
              collapsed={collapsed.has("section:recents")}
              onToggle={() => {
                toggleProject("section:recents");
              }}
              actions={
                <SidebarSortMenu
                  label="Recents"
                  value={sidebarSort.recents}
                  onChange={(value) => {
                    setSidebarSort((previous) => ({ ...previous, recents: value }));
                  }}
                />
              }
            >
              {sortThreads(
                filtered.filter((t) => !t.projectId && !t.pinned),
                sidebarSort.recents,
              ).map(threadRow)}
            </SidebarSection>
          )}
        </div>
      </div>
      <div className="sidebar-footer">
        <ContributionSlot name="sidebar.footer" />
        <output className="workspace-status">
          <span className={`connection-dot ${state.connected ? "connected" : ""}`} />
          <span>
            {(() => {
              if (state.cacheSaving) {
                return "Saving draft…";
              }
              if (state.connected) {
                return (
                  nodes.nodes.find(
                    (node) => node.identity.environmentId === state.node?.environmentId,
                  )?.displayName ?? "Connected"
                );
              }
              if (state.busy) {
                return "Connecting…";
              }
              return "Disconnected";
            })()}
          </span>
        </output>
      </div>
    </aside>
  );
}

function ThreadRow({
  thread,
  view,
}: {
  thread: Thread;
  view: Pick<
    WorkspaceViewState,
    | "store"
    | "state"
    | "setUndo"
    | "setRenaming"
    | "setTitle"
    | "setSidebarSort"
    | "active"
    | "perform"
    | "select"
  >;
}) {
  const { store, state, setUndo, setRenaming, setTitle, setSidebarSort, active, perform, select } =
    view;
  const threadDrag = useRef<
    { id: string; x: number; y: number; moved: boolean; before?: string } | undefined
  >(undefined);
  const draggedClick = useRef<string | undefined>(undefined);
  return (
    <div
      key={thread.id}
      className={`group/chat-row relative rounded-lg ${active.id === thread.id ? "bg-control-secondary" : "hover:bg-control-hover focus-within:bg-control-hover"}`}
    >
      <Button
        variant="thread"
        data-thread-id={thread.id}
        onClick={() => {
          if (draggedClick.current === thread.id) {
            draggedClick.current = undefined;
            return;
          }
          select(thread.id);
        }}
        title={thread.title}
        onPointerDown={(e) => {
          draggedClick.current = undefined;
          if (e.button !== 0 || e.pointerType === "touch") {
            return;
          }
          threadDrag.current = {
            id: thread.id,
            x: e.clientX,
            y: e.clientY,
            moved: false,
          };
          e.currentTarget.setPointerCapture(e.pointerId);
        }}
        onPointerMove={(e) => {
          const drag = threadDrag.current;
          if (!drag || drag.id !== thread.id) {
            return;
          }
          if (!drag.moved && Math.hypot(e.clientX - drag.x, e.clientY - drag.y) < 8) {
            return;
          }
          drag.moved = true;
          drag.before = document
            .elementFromPoint(e.clientX, e.clientY)
            ?.closest<HTMLElement>("[data-thread-id]")?.dataset.threadId;
        }}
        onPointerCancel={() => {
          threadDrag.current = undefined;
        }}
        onPointerUp={(e) => {
          const drag = threadDrag.current;
          threadDrag.current = undefined;
          if (e.currentTarget.hasPointerCapture(e.pointerId)) {
            e.currentTarget.releasePointerCapture(e.pointerId);
          }
          if (!(drag?.moved === true)) {
            return;
          }
          draggedClick.current = drag.id;
          if (!hasText(drag.before) || drag.before === drag.id) {
            return;
          }
          setSidebarSort((previous) => ({
            ...previous,
            [thread.pinned ? "pinned" : "recents"]: "manual",
          }));
          const previous = state.threads.map((entry) => entry.id);
          store.reorderThread(drag.id, drag.before);
          setUndo({
            title: "Conversation order changed",
            run: () => {
              store.orderThreads(previous);
              return undefined;
            },
          });
        }}
      >
        <span>{thread.title}</span>
        {["starting", "running", "needs_input"].includes(thread.state) && (
          <span className="status-dot working" />
        )}
      </Button>
      <SidebarChatActions
        thread={thread}
        onPin={() => {
          store.updateThread(thread.id, { pinned: !thread.pinned }, false);
        }}
        onRename={() => {
          select(thread.id);
          setTitle(thread.title);
          setRenaming(true);
        }}
        onArchive={() => {
          startAction(
            perform(async () => {
              const previous = thread.archived === true;
              await store.archive(thread.id, !previous);
              setUndo({
                title: previous ? "Conversation restored" : "Conversation archived",
                run: async () => {
                  await store.archive(thread.id, previous);
                },
              });
            }),
            store.setError.bind(store),
          );
        }}
      />
    </div>
  );
}
