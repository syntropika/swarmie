import {
  BrandWordmark,
  Button,
  Dropdown,
  ResizeHandle,
  Tooltip,
  TooltipContent,
} from "@swarmie/ui-web";
import { ChevronDown, Search, SquarePen, Plus, FolderOpen, MoreHorizontal } from "lucide-react";
// Copyright 2026 Syntropika
// SPDX-License-Identifier: Apache-2.0
import { useRef } from "react";

import { startAction } from "~/src/action-boundary";
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
    | "state"
    | "sidebarWidth"
    | "setSidebarWidth"
    | "organization"
    | "now"
    | "archived"
    | "searchScope"
    | "setSearchScope"
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
    | "active"
    | "perform"
    | "select"
  >;
}) {
  const {
    sidebarWidth,
    setSidebarWidth,
    organization,
    now,
    archived,
    searchScope,
    setSearchScope,
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
        data-workspace-part="sidebar-resize"

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
        <div className="pl-3" data-workspace-part="workspace-wordmark">
          <span className="sr-only">Swarmie</span>
          <BrandWordmark className="w-24" />
        </div>
        <IconButton
          label="Search chats"
          active={searchScope !== null}
          onClick={() => {
            setSearchScope("chats");
          }}
        >
          <Search size={16} />
        </IconButton>
      </div>
      <div className="sidebar-navigation">
        <Button
          variant="tertiary"
          data-workspace-part="sidebar-nav-item"
          onClick={() => {
            newChat();
          }}
        >
          <SquarePen size={16} />
          <span>New chat</span>
        </Button>
      </div>
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
                      variant="tertiary"
                      size="sm"
                      isIconOnly
                      aria-label="Add new project"

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
                    variant="tertiary"
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
                      variant="tertiary"
                      size="sm"
                      isIconOnly
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
                        variant="tertiary"
                        size="sm"
                        isIconOnly
                        aria-label={`New chat in ${p.name}`}

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
    | "setSidebarSort"
    | "active"
    | "perform"
    | "select"
  >;
}) {
  const { store, state, setUndo, setRenaming, setSidebarSort, active, perform, select } = view;
  const threadDrag = useRef<
    { id: string; x: number; y: number; moved: boolean; before?: string } | undefined
  >(undefined);
  const draggedClick = useRef<string | undefined>(undefined);
  return (
    <div
      key={thread.id}
      className={`group/chat-row relative rounded-lg ${active.id === thread.id ? "bg-default" : "focus-within:bg-default-hover hover:bg-default-hover"}`}
    >
      <Button
        variant="tertiary"
        data-workspace-part="thread-row"
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
          setRenaming({ id: thread.id, title: thread.title });
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
