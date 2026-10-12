import {
  useEffect,
  useRef,
  useState,
  useSyncExternalStore,
  lazy,
  Suspense,
} from "react";
import {
  History,
  GitBranch,
  PanelLeft,
  Settings,
  Search,
  Plus,
  Folder,
  FolderOpen,
  ChevronDown,
  MoreHorizontal,
  SquarePen,
  X,
  RefreshCw,
  ShieldCheck,
  ArrowLeft,
  ArrowRight,
  Check,
  Plug,
  Trash2,
  Archive,
  Pencil,
  Command,
  Layers,
  PanelRight,
  Home,
  Monitor,
  Pin,
  PinOff,
} from "lucide-react";
import { Dropdown } from "@/components/ui/dropdown";
import { Button } from "@/components/ui/button";
import { Select } from "@/components/ui/select";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import {
  Tooltip,
  TooltipContent,
} from "@/components/ui/tooltip";
import { Conversation, PatchCopy } from "./Conversation";
import { nodeClient } from "./workspaceInstance";
import type { NodeClient, NodesState } from "./nodes";
import type { WorkspaceStore } from "./workspace";
import { nodeKey } from "./nodeScope";
import { hasConversation, type Thread, type Permission } from "./protocol";

import { RequestCard } from "./RequestCard";
import { SessionControls } from "./SessionControls";
import { ContextUsage } from "./ContextUsage";
import { SettingsPage } from "./SettingsPage";
import { HarnessSelect } from "./HarnessSelect";
const ExtensionPanel = lazy(() =>
  import("./ExtensionPanel").then((module) => ({
    default: module.ExtensionPanel,
  })),
);
import { CommandPalette } from "./CommandPalette";
import { FileFinder } from "./FileFinder";
import {
  readShortcuts,
  matchesShortcut,
} from "./shortcuts";
import { ProjectActions } from "./ProjectActions";
import { ProjectDialog } from "./ProjectDialog";
import { watchThreadTimes } from "./threadClock";
import { SidebarSection, SidebarSortMenu, type SidebarSort } from "./SidebarSection";
import { SidebarChatActions } from "./SidebarChatActions";

type Panel = "project" | "settings" | "review" | "extensions" | null;
const blank: Thread = {
  id: "blank",
  title: "New chat",
  projectId: "",
  worktreeId: "",
  harness: "codex",
  model: "",
  state: "idle",
  sequence: 0,
  messages: [],
  tools: [],
  permissions: [],
  pinned: false,
};

function IconButton({
  label,
  children,
  onClick,
  active = false,
  disabled = false,
  className = "",
}: {
  label: string;
  children: React.ReactNode;
  onClick?: () => void;
  active?: boolean;
  disabled?: boolean;
  className?: string;
}) {
  return (
    <Tooltip delay={500}>
        <Button
          variant={active ? "secondary" : "ghost"}
          size="icon"
          aria-label={label}
          className={`icon-button ${active ? "active" : ""} ${className}`}
          onClick={onClick}
          disabled={disabled}
        >
          {children}
        </Button>
      <TooltipContent placement="bottom" offset={6}>
        {label}
      </TooltipContent>
    </Tooltip>
  );
}

export function App() {
  const nodes = useSyncExternalStore(nodeClient.subscribe, nodeClient.getSnapshot);
  useEffect(() => { void nodeClient.start(); return () => nodeClient.dispose(); }, []);
  const node = nodes.store.getSnapshot().node;
  return <NodeWorkspace key={node ? nodeKey(node) : "local-startup"} store={nodes.store} nodes={nodes} client={nodeClient} />;
}
function NodeWorkspace({ store, nodes, client }: { store: WorkspaceStore; nodes: NodesState; client: NodeClient }) {
  const state = useSyncExternalStore(store.subscribe, store.getSnapshot);
  const [sidebar, setSidebar] = useState(() => window.innerWidth >= 760);
  const [sidebarWidth, setSidebarWidth] = useState(() => {
    try {
      const saved = Number(localStorage.getItem("swarmie.sidebar-width"));
      return saved >= 220 && saved <= 360 ? saved : 290;
    } catch {
      return 290;
    }
  });
  const [panel, setPanel] = useState<Panel>(null);
  const [palette, setPalette] = useState(false);
  const [finder, setFinder] = useState(false);
  const [shortcuts, setShortcuts] = useState(readShortcuts);
  const [organization, setOrganization] = useState<
    "projects" | "time" | "working"
  >(() => {
    try {
      const saved = localStorage.getItem("swarmie.organization");
      return saved === "time" || saved === "working" ? saved : "projects";
    } catch {
      return "projects";
    }
  });
  const [now, setNow] = useState(Date.now);
  const [showSnoozed, setShowSnoozed] = useState(false);
  const [undo, setUndo] = useState<{
    title: string;
    run: () => Promise<unknown> | void;
  }>();
  const [archived, setArchived] = useState(false);
  const [renaming, setRenaming] = useState(false);
  const [title, setTitle] = useState("");
  const [query, setQuery] = useState("");
  const [search, setSearch] = useState(false);
  const [projectDialog, setProjectDialog] = useState(false);
  const [branch, setBranch] = useState("");
  const [working, setWorking] = useState(false);
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [sidebarSort, setSidebarSort] = useState<Record<"projects" | "recents" | "pinned", SidebarSort>>(() => {
    const defaults = { projects: "manual", recents: "updated_at", pinned: "manual" } as const;
    try {
      const saved = JSON.parse(localStorage.getItem("swarmie.sidebar-sorting") ?? "null");
      return Object.fromEntries(Object.entries(defaults).map(([key, value]) => [key, ["manual", "updated_at"].includes(saved?.[key]) ? saved[key] : value])) as typeof defaults;
    } catch { return defaults; }
  });
  useEffect(() => {
    try { localStorage.setItem("swarmie.sidebar-sorting", JSON.stringify(sidebarSort)); }
    catch (error) { store.setError(error); }
  }, [sidebarSort]);
  const history = useRef<string[]>([]);
  const historyIndex = useRef(-1);
  const threadDrag = useRef<
    | { id: string; x: number; y: number; moved: boolean; before?: string }
    | undefined
  >(undefined);
  const draggedClick = useRef<string | undefined>(undefined);
  const active = state.threads.find((t) => t.id === state.activeId) ?? blank;
  const project = state.projects.find((p) => p.id === active.projectId);
  const tree = project?.worktrees.find((t) => t.id === active.worktreeId);
  const sessionOpen =
    !!active.sessionId && !["closed", "failed"].includes(active.state);
  const ready =
    state.connected &&
    !!active.worktreeId &&
    state.harnesses.some((h) => h.harness === active.harness && h.available);
  const filtered = state.threads.filter(
    (t) =>
      hasConversation(t) &&
      !!t.archived === archived &&
      (showSnoozed ||
        (!t.settled && (!t.snoozeUntil || t.snoozeUntil <= now))) &&
      (t.title.toLowerCase().includes(query.toLowerCase()) ||
        t.messages.some((m) =>
          typeof m.content === "string"
            ? m.content.toLowerCase().includes(query.toLowerCase())
            : m.content.some(
                (p) =>
                  p.type === "text" &&
                  p.text.toLowerCase().includes(query.toLowerCase()),
              ),
        )),
  );
  const sortThreads = (threads: Thread[], sort: SidebarSort) => sort === "manual" ? threads : [...threads].sort((a, b) => (b.updatedAtMs ?? b.createdAtMs ?? 0) - (a.updatedAtMs ?? a.createdAtMs ?? 0));
  const pinned = sortThreads(filtered.filter((t) => t.pinned), sidebarSort.pinned);
  const projects = sidebarSort.projects === "manual" ? state.projects : [...state.projects].sort((a, b) => {
    const activity = (id: string) => Math.max(0, ...state.threads.filter((thread) => thread.projectId === id).map((thread) => thread.updatedAtMs ?? thread.createdAtMs ?? 0));
    return activity(b.id) - activity(a.id);
  });
  useEffect(() => watchThreadTimes(state.threads, setNow), [state.threads]);
  useEffect(() => {
    try {
      localStorage.setItem("swarmie.sidebar-width", String(sidebarWidth));
    } catch (error) {
      store.setError(error);
    }
  }, [sidebarWidth]);
  useEffect(() => {
    try {
      localStorage.setItem("swarmie.organization", organization);
    } catch (error) {
      store.setError(error);
    }
  }, [organization]);
  const navigationApplied = useRef(false);
  const applyNavigation = async () => {
    if (store.getSnapshot().connected && !navigationApplied.current) {
        navigationApplied.current = true;
        const query = new URLSearchParams(window.location.search);
        const project = query.get("project");
        const session = query.get("session");
        if (project && !(await store.openProject(project))) return;
        const thread = store
          .getSnapshot()
          .threads.find((t) => t.sessionId === session);
        if (thread) store.select(thread.id);
        navigationApplied.current = true;
    }
  };
  useEffect(() => {
    if (state.connected && !navigationApplied.current && nodes.nodes.find((node) => node.identity.environmentId === state.node?.environmentId)?.local) {
      void applyNavigation().catch((error) => store.setError(error));
    }
  }, [state.connected, state.node, nodes.nodes]);
  useEffect(() => {
    const compact = window.matchMedia("(max-width: 760px)");
    const adapt = () => {
      if (compact.matches) setSidebar(false);
    };
    compact.addEventListener("change", adapt);
    return () => compact.removeEventListener("change", adapt);
  }, []);
  useEffect(() => {
    const opened = () => setPanel("extensions");
    window.addEventListener("swarmie:open-tools", opened);
    return () => window.removeEventListener("swarmie:open-tools", opened);
  }, []);
  useEffect(() => {
    const saved = active.layout?.panel;
    setPanel((current) => current === "settings" ? current
      : saved === null || ["project", "settings", "review", "extensions"].includes(saved ?? "")
        ? saved as Panel : null);
  }, [active.id]);
  useEffect(() => {
    if (active !== blank && active.layout?.panel !== panel)
      store.updateThread(active.id, { layout: { ...active.layout, panel } }, false);
  }, [panel]);
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.defaultPrevented) return;
      if (matchesShortcut(event, shortcuts.palette)) {
        event.preventDefault();
        setPalette((v) => !v);
      }
      if (matchesShortcut(event, shortcuts.newChat)) {
        event.preventDefault();
        store.createThread();
        setPanel(null);
      }
      if (matchesShortcut(event, shortcuts.files)) {
        event.preventDefault();
        setFinder(true);
      }
      if (matchesShortcut(event, shortcuts.tools)) {
        event.preventDefault();
        setPanel((p) => (p === "extensions" ? null : "extensions"));
      }
      if (event.key === "Escape") {
        setPanel(null);
        setSearch(false);
        setQuery("");
        if (window.innerWidth <= 760) setSidebar(false);
        setPalette(false);
        setFinder(false);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [active.projectId, shortcuts]);
  const perform = async (action: () => Promise<unknown>) => {
    setWorking(true);
    try {
      await action();
    } catch (e) {
      store.setError(e);
    } finally {
      setWorking(false);
    }
  };
  const select = (id: string) => {
    history.current = [
      ...history.current.slice(0, historyIndex.current + 1),
      id,
    ];
    historyIndex.current = history.current.length - 1;
    store.select(id);
    if (window.innerWidth < 760) setSidebar(false);
  };
  const navigate = (direction: number) => {
    const index = historyIndex.current + direction;
    if (index >= 0 && index < history.current.length) {
      historyIndex.current = index;
      store.select(history.current[index]);
    }
  };
  const newChat = (projectId?: string) => {
    store.createThread(projectId);
    setPanel(null);
    if (window.innerWidth < 760) setSidebar(false);
  };
  const renameActive = () => {
    setTitle(active.title);
    setRenaming(true);
  };
  const archiveDisabled =
    active === blank ||
    ["running", "needs_input", "starting"].includes(active.state);
  const archiveActive = () => {
    const id = active.id,
      previous = !!active.archived;
    void perform(async () => {
      await store.archive(id, !previous);
      setUndo({
        title: previous ? "Conversation restored" : "Conversation archived",
        run: () => store.archive(id, previous),
      });
    });
  };
  const pinActive = () =>
    store.updateThread(active.id, { pinned: !active.pinned });
  const threadRow = (thread: Thread) => (
    <div key={thread.id} className={`group/chat-row relative rounded-lg ${active.id === thread.id ? "bg-control-secondary" : "hover:bg-control-hover focus-within:bg-control-hover"}`}>
    <Button
      variant="ghost"
      className="thread-row justify-start pr-16 hover:bg-transparent"
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
        if (e.button !== 0 || e.pointerType === "touch") return;
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
        if (!drag || drag.id !== thread.id) return;
        if (
          !drag.moved &&
          Math.hypot(e.clientX - drag.x, e.clientY - drag.y) < 8
        )
          return;
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
        if (e.currentTarget.hasPointerCapture(e.pointerId))
          e.currentTarget.releasePointerCapture(e.pointerId);
        if (!drag?.moved) return;
        draggedClick.current = drag.id;
        if (!drag.before || drag.before === drag.id) return;
        setSidebarSort((previous) => ({ ...previous, [thread.pinned ? "pinned" : "recents"]: "manual" }));
        const previous = state.threads.map((entry) => entry.id);
        store.reorderThread(drag.id, drag.before);
        setUndo({
          title: "Conversation order changed",
          run: () => store.orderThreads(previous),
        });
      }}
    >
      <span>{thread.title}</span>
      {["starting", "running", "needs_input"].includes(thread.state) && (
        <span className="status-dot working" />
      )}
    </Button>
    <SidebarChatActions thread={thread}
      onPin={() => store.updateThread(thread.id, { pinned: !thread.pinned }, false)}
      onRename={() => { select(thread.id); setTitle(thread.title); setRenaming(true); }}
      onArchive={() => void perform(async () => {
        const previous = !!thread.archived;
        await store.archive(thread.id, !previous);
        setUndo({ title: previous ? "Conversation restored" : "Conversation archived", run: () => store.archive(thread.id, previous) });
      })} />
    </div>
  );
  const toggleProject = (id: string) =>
    setCollapsed((previous) => {
      const next = new Set(previous);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  const composerContext = (
    <>
      <HarnessSelect label="Harness" appearance="menu" className="composer-harness" status={{ state: active.state, connected: state.connected }} disabled={!!active.sessionId || !state.connected}
        value={active.harness} onChange={(harness) => store.updateThread(active.id, { harness, model: "", config: {}, configurations: undefined, models: undefined, actualModel: undefined })} harnesses={state.harnesses} />
      <div className="composer-spacer" />
      <Select label="View node" className="composer-host max-w-[50%] h-7 rounded-full border-0 bg-transparent px-2.5 text-xs hover:bg-[#4a4a4a]" value={state.node?.environmentId ?? ""} icon={<Monitor size={14} />} options={nodes.nodes.filter((node) => node.registered || node.identity.environmentId === state.node?.environmentId).map((node) => ({ id: node.identity.environmentId, label: node.displayName, disabled: !node.registered }))} onChange={(id) => void client.select(id).catch((error) => store.setError(error))} />
    </>
  );
  const composerControls = <><ContextUsage context={active.context} /><SessionControls thread={active} store={store} connected={state.connected} /></>;

  return (
    <div
      className={`app-shell ${sidebar ? "" : "sidebar-hidden"} ${panel === "settings" ? "settings-open" : ""}`}
      style={
        {
          "--panel-width": `${active.layout?.width ?? 370}px`,
          "--sidebar-width": `${sidebarWidth}px`,
        } as React.CSSProperties
      }
    >
      <header className="titlebar">
        <div className="titlebar-navigation">
          <IconButton
            label="Back"
            onClick={() => navigate(-1)}
            disabled={historyIndex.current < 1}
          >
            <ArrowLeft size={14} />
          </IconButton>
          <IconButton
            label="Forward"
            onClick={() => navigate(1)}
            disabled={historyIndex.current >= history.current.length - 1}
          >
            <ArrowRight size={14} />
          </IconButton>
          <IconButton
            label="Toggle sidebar"
            className="toggle-sidebar"
            onClick={() => { if (window.innerWidth <= 1100 && panel) setPanel(null); setSidebar((v) => !v); }}
          >
            <PanelLeft size={15} />
          </IconButton>
        </div>
        <div className="titlebar-topic">
          {panel === "settings" ? <Settings size={14} /> : <Folder size={14} />}
          {renaming ? (
            <form
              onSubmit={(e) => {
                e.preventDefault();
                void perform(async () => {
                  await store.rename(active.id, title);
                  setRenaming(false);
                });
              }}
            >
              <Input
                aria-label="Conversation title"
                autoFocus
                value={title}
                onChange={(e) => setTitle(e.target.value)}
                onBlur={() => {
                  void perform(() => store.rename(active.id, title));
                  setRenaming(false);
                }}
              />
            </form>
          ) : (
            <h1>
              {panel === "settings" ? "Settings" : active.title === "New chat" && project
                ? project.name
                : active.title}
            </h1>
          )}
        </div>
        <div className="titlebar-actions">
          <IconButton label="Workspace tools" active={panel === "extensions"}
            disabled={!state.extensions.some((e) => e.views?.length)}
            onClick={() => setPanel(panel === "extensions" ? null : "extensions")}>
            <PanelRight size={16} />
          </IconButton>
          <IconButton label="Review changes" active={panel === "review"} disabled={!project?.git}
            onClick={() => { setPanel(panel === "review" ? null : "review"); void perform(() => store.review()); }}>
            <GitBranch size={16} />
          </IconButton>
          <Dropdown>
            <Button variant="ghost" size="icon" className="icon-button" aria-label="Conversation actions"><MoreHorizontal size={17} /></Button>
            <Dropdown.Popover className="workspace-menu" placement="bottom end">
              <Dropdown.Menu aria-label="Conversation actions">
                <Dropdown.Item id="rename" textValue="Rename chat" isDisabled={active === blank} onAction={renameActive}><Pencil size={14} />Rename chat</Dropdown.Item>
                <Dropdown.Item id="pin" textValue={active.pinned ? "Unpin chat" : "Pin chat"} isDisabled={active === blank} onAction={pinActive}>{active.pinned ? <PinOff size={14} /> : <Pin size={14} />}{active.pinned ? "Unpin chat" : "Pin chat"}</Dropdown.Item>
                <Dropdown.Item id="archive" textValue={active.archived ? "Restore chat" : "Archive chat"} isDisabled={archiveDisabled} onAction={archiveActive}><Archive size={14} />{active.archived ? "Restore chat" : "Archive chat"}</Dropdown.Item>
                <Dropdown.Item id="settle" textValue={active.settled ? "Reopen conversation" : "Mark settled"} isDisabled={active === blank}
                  onAction={() => {
                    const previous = { settled: active.settled, snoozeUntil: active.snoozeUntil };
                    store.updateThread(active.id, { settled: !active.settled });
                    setUndo({ title: "Conversation updated", run: () => store.updateThread(active.id, previous) });
                  }}><Check size={14} />{active.settled ? "Reopen conversation" : "Mark settled"}</Dropdown.Item>
                <Dropdown.Item id="snooze" textValue={active.snoozeUntil ? "Unsnooze" : "Snooze for one hour"} isDisabled={active === blank}
                  onAction={() => {
                    const previous = active.snoozeUntil;
                    store.updateThread(active.id, { snoozeUntil: active.snoozeUntil ? undefined : Date.now() + 3600_000 });
                    setUndo({ title: "Snooze updated", run: () => store.updateThread(active.id, { snoozeUntil: previous }) });
                  }}><History size={14} />{active.snoozeUntil ? "Unsnooze" : "Snooze for one hour"}</Dropdown.Item>
                <Dropdown.Item id="files" textValue="Find file" onAction={() => setFinder(true)}><Search size={14} />Find file</Dropdown.Item>
                <Dropdown.Item id="commands" textValue="Command palette" onAction={() => setPalette(true)}><Command size={14} />Command palette</Dropdown.Item>
              </Dropdown.Menu>
            </Dropdown.Popover>
          </Dropdown>
        </div>
      </header>
      <nav className="activity-rail" aria-label="Workspace navigation">
        <IconButton label="Chats" active={!panel && !archived} onClick={() => { setPanel(null); setArchived(false); setSidebar(true); }}><Home size={18} /></IconButton>
        <IconButton label="Archived chats" active={archived} onClick={() => { setArchived((value) => !value); setSidebar(true); }}><History size={18} /></IconButton>
        <div className="rail-divider" />
        <IconButton label="Review changes" active={panel === "review"} disabled={!project?.git} onClick={() => { setPanel(panel === "review" ? null : "review"); void perform(() => store.review()); }}><GitBranch size={18} /></IconButton>
        <IconButton label="Workspace tools" active={panel === "extensions"} disabled={!state.extensions.some((e) => e.views?.length)} onClick={() => setPanel(panel === "extensions" ? null : "extensions")}><Layers size={18} /></IconButton>
        <div className="rail-spacer" />
        <IconButton label="Settings" active={panel === "settings"} onClick={() => setPanel(panel === "settings" ? null : "settings")}><Settings size={18} /></IconButton>
      </nav>
      <aside className="sidebar" aria-label="Projects and chats">
        <div className="sidebar-resize" role="separator" aria-label="Resize sidebar" aria-orientation="vertical"
          aria-valuemin={220} aria-valuemax={360} aria-valuenow={sidebarWidth} tabIndex={0}
          onKeyDown={(event) => {
            if (event.key === "ArrowLeft" || event.key === "ArrowRight") {
              event.preventDefault();
              setSidebarWidth((width) => Math.max(220, Math.min(360, width + (event.key === "ArrowRight" ? 20 : -20))));
            }
          }}
          onPointerDown={(event) => event.currentTarget.setPointerCapture(event.pointerId)}
          onPointerMove={(event) => {
            if (event.currentTarget.hasPointerCapture(event.pointerId)) setSidebarWidth(Math.max(220, Math.min(360, event.clientX - 50)));
          }}
          onPointerUp={(event) => event.currentTarget.releasePointerCapture(event.pointerId)}
        />
        <div className="sidebar-heading">
          <Dropdown>
            <Button variant="ghost" className="workspace-wordmark" aria-label="Workspace menu">swarmie<ChevronDown size={12} /></Button>
            <Dropdown.Popover className="workspace-menu">
              <Dropdown.Menu aria-label="Workspace organization">
                <Dropdown.Item id="projects" textValue="Group by project" onAction={() => setOrganization("projects")}>Projects{organization === "projects" && <Check size={14} />}</Dropdown.Item>
                <Dropdown.Item id="time" textValue="Group by recent activity" onAction={() => setOrganization("time")}>Recent activity{organization === "time" && <Check size={14} />}</Dropdown.Item>
                <Dropdown.Item id="working" textValue="Group by working status" onAction={() => setOrganization("working")}>Working{organization === "working" && <Check size={14} />}</Dropdown.Item>
                <Dropdown.Item id="settled" textValue={showSnoozed ? "Hide settled conversations" : "Show all conversations"} onAction={() => setShowSnoozed((value) => !value)}>{showSnoozed ? "Hide settled conversations" : "Show all conversations"}</Dropdown.Item>
              </Dropdown.Menu>
            </Dropdown.Popover>
          </Dropdown>
          <IconButton label="Search chats" active={search} onClick={() => { setSearch((value) => !value); if (search) setQuery(""); }}><Search size={16} /></IconButton>
        </div>
        <div className="sidebar-navigation">
          <Button variant="ghost" className="sidebar-nav-item" onClick={() => newChat()}><SquarePen size={16} /><span>New chat</span></Button>
        </div>
        {search && (
          <Input
            className="sidebar-search"
            autoFocus
            aria-label="Search chats"
            placeholder="Search chats…"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
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
                      [
                        "starting",
                        "running",
                        "needs_input",
                        "cancelling",
                      ].includes(t.state),
                    )
                    .map(threadRow)}
                  <h2>Other conversations</h2>
                  {filtered
                    .filter(
                      (t) =>
                        ![
                          "starting",
                          "running",
                          "needs_input",
                          "cancelling",
                        ].includes(t.state),
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
                        return index === 0
                          ? age < 86400_000
                          : index === 1
                            ? age >= 86400_000 && age < 7 * 86400_000
                            : age >= 7 * 86400_000;
                      })
                      .map(threadRow)}
                  </div>
                ))
              )}
            </section>
          )}
          <div hidden={organization !== "projects"}>
            {pinned.length > 0 && (
              <SidebarSection label="Pinned" collapsed={collapsed.has("section:pinned")} onToggle={() => toggleProject("section:pinned")}
                actions={<SidebarSortMenu label="Pinned" value={sidebarSort.pinned} onChange={(value) => setSidebarSort((previous) => ({ ...previous, pinned: value }))} />}>
                {pinned.map(threadRow)}
              </SidebarSection>
            )}
            <SidebarSection label={archived ? "Archived chats" : "Projects"} collapsed={collapsed.has("section:projects")} onToggle={() => toggleProject("section:projects")}
              actions={<>
                <SidebarSortMenu label="Projects" value={sidebarSort.projects} onChange={(value) => setSidebarSort((previous) => ({ ...previous, projects: value }))} />
                {!archived && <Tooltip><Button variant="ghost" size="icon-sm" aria-label="Add new project" className="text-muted" onClick={() => setProjectDialog(true)}><Plus size={14} /></Button><TooltipContent>Add new project</TooltipContent></Tooltip>}
              </>}>
              {projects.map((p) => (
                  <div className="project-group group/project" key={p.id}>
                    <div className="project-heading">
                      <Button
                        variant="ghost"
                        onClick={() => toggleProject(p.id)}
                      >
                        <span className="project-icon">
                          <FolderOpen size={14} />
                          <span />
                        </span>
                        <span className="project-name">{p.name}</span>
                        <ChevronDown size={12} className={`shrink-0 transition-transform ${collapsed.has(p.id) ? "-rotate-90" : "opacity-0 group-hover/project:opacity-100 group-focus-within/project:opacity-100"}`} />
                      </Button>
                      <div className="pointer-events-none opacity-0 group-hover/project:pointer-events-auto group-hover/project:opacity-100 group-focus-within/project:pointer-events-auto group-focus-within/project:opacity-100">
                        <Tooltip><Button variant="ghost" size="icon-sm" aria-label={`New chat in ${p.name}`} className="text-muted" onClick={() => newChat(p.id)}><Plus size={14} /></Button><TooltipContent>New chat</TooltipContent></Tooltip>
                      </div>
                    </div>
                    {!collapsed.has(p.id) && (
                      <div className="project-chats">
                        {sortThreads(filtered.filter((t) => t.projectId === p.id && !t.pinned), sidebarSort.recents).map(threadRow)}
                        {!filtered.some((t) => t.projectId === p.id) && (
                          <p className="no-chats">No chats yet</p>
                        )}
                      </div>
                    )}
                  </div>
                ))}
            </SidebarSection>
            {filtered.some((t) => !t.projectId && !t.pinned) && (
              <SidebarSection label="Recents" collapsed={collapsed.has("section:recents")} onToggle={() => toggleProject("section:recents")}
                actions={<SidebarSortMenu label="Recents" value={sidebarSort.recents} onChange={(value) => setSidebarSort((previous) => ({ ...previous, recents: value }))} />}>
                {sortThreads(filtered.filter((t) => !t.projectId && !t.pinned), sidebarSort.recents).map(threadRow)}
              </SidebarSection>
            )}
          </div>
        </div>
        <div className="sidebar-footer">
          <div className="workspace-status" role="status">
            <span className={`connection-dot ${state.connected ? "connected" : ""}`} />
            <span>{state.cacheSaving ? "Saving draft…" : state.connected ? (nodes.nodes.find((node) => node.identity.environmentId === state.node?.environmentId)?.displayName ?? "Connected") : state.busy ? "Connecting…" : "Disconnected"}</span>
          </div>
        </div>
      </aside>
      {sidebar && <Button variant="ghost" className="sidebar-backdrop" aria-label="Close sidebar" onClick={() => setSidebar(false)} />}
      <main className="main-content">
        {undo && (
          <div className="undo-banner" role="status">
            <span>{undo.title}</span>
            <Button
              variant="ghost"
              className="small-button"
              onClick={() => {
                void Promise.resolve(undo.run()).catch((e) =>
                  store.setError(e),
                );
                setUndo(undefined);
              }}
            >
              Undo
            </Button>
            <Button
              variant="ghost"
              className="icon-button"
              aria-label="Dismiss undo"
              onClick={() => setUndo(undefined)}
            >
              <X size={12} />
            </Button>
          </div>
        )}
        {state.error && (
          <div className="error-banner" role="alert">
            <span>{state.error}</span>
            <IconButton
              label="Dismiss error"
              onClick={() => store.dismissError()}
            >
              <X size={14} />
            </IconButton>
          </div>
        )}
        {active.permissions.map((p) => (
          <RequestCard
            key={p.id}
            permission={p}
            onAnswer={(option, data) => store.respond(p.id, option, data)}
          />
        ))}
        <Conversation
          key={active.id}
          thread={active}
          store={store}
          controls={composerControls}
          permissions={<SessionControls kind="settings" thread={active} store={store} connected={state.connected} />}
          context={composerContext}
          projectName={project?.name}
          ready={ready}
        />
      </main>
      {palette && (
        <CommandPalette
          store={store}
          onClose={() => setPalette(false)}
          onPanel={setPanel}
          onOpenProject={() => { setPanel(null); setProjectDialog(true); }}
        />
      )}
      {finder && <FileFinder store={store} onClose={() => setFinder(false)} />}
      {projectDialog && <ProjectDialog store={store} onClose={() => setProjectDialog(false)} />}
      {panel === "settings" && <SettingsPage client={client} nodes={nodes} store={store} state={state} shortcuts={shortcuts} setShortcuts={setShortcuts} onClose={() => setPanel(null)} />}
      {panel && panel !== "settings" && (
        <aside
          className="detail-panel"
          aria-label={
            panel === "review"
              ? "Change review"
              : panel === "project"
                ? "Project settings"
                : panel === "extensions"
                  ? "Workspace tools"
                  : "Connection settings"
          }
        >
          <div
            className="panel-resize"
            role="separator"
            aria-label="Resize workspace panel"
            aria-orientation="vertical"
            tabIndex={0}
            onKeyDown={(e) => {
              if (["ArrowLeft", "ArrowRight"].includes(e.key)) {
                e.preventDefault();
                store.updateThread(active.id, {
                  layout: {
                    ...active.layout,
                    width: Math.max(
                      300,
                      Math.min(
                        800,
                        (active.layout?.width ?? 370) +
                          (e.key === "ArrowLeft" ? 20 : -20),
                      ),
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
              if (e.currentTarget.hasPointerCapture(e.pointerId))
                store.updateThread(active.id, {
                  layout: {
                    ...active.layout,
                    width: Math.max(
                      300,
                      Math.min(800, window.innerWidth - e.clientX),
                    ),
                    panel,
                  },
                });
            }}
            onPointerUp={(e) =>
              e.currentTarget.releasePointerCapture(e.pointerId)
            }
          />
          <div className="detail-heading">
            <h2>
              {panel === "review"
                ? "Changes"
                : panel === "project"
                  ? "Project"
                  : panel === "extensions"
                    ? "Workspace tools"
                    : "Settings"}
            </h2>
            <IconButton label="Close panel" onClick={() => setPanel(null)}>
              <X size={17} />
            </IconButton>
          </div>
          {panel === "extensions" && (
            <Suspense
              fallback={<p className="panel-note">Loading workspace tools…</p>}
            >
              <ExtensionPanel
                key={active.id}
                store={store}
                thread={active}
                extensions={state.extensions}
              />
            </Suspense>
          )}
          {panel === "project" && (
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
                  <label>
                    Worktree
                    <Select label="Worktree" disabled={!!active.sessionId} value={active.worktreeId}
                      onChange={(worktreeId) => store.updateThread(active.id, { worktreeId })}
                      options={project.worktrees.filter((w) => !w.prunable).map((w) => ({ id: w.id, label: (w.branch ?? (project.git ? "Detached HEAD" : "Local folder")) + (w.isMain ? " · main worktree" : "") }))} />
                  </label>
                  <form
                    onSubmit={(e) => {
                      e.preventDefault();
                      void perform(async () => {
                        await store.createWorktree(branch);
                        setBranch("");
                      });
                    }}
                  >
                    <label>
                      New branch
                      <Input
                        aria-label="New branch"
                        value={branch}
                        onChange={(e) => setBranch(e.target.value)}
                        placeholder="feature/my-task"
                        disabled={!!active.sessionId}
                      />
                    </label>
                    <Button
                      variant="ghost"
                      className="small-button"
                      type="submit"
                      disabled={
                        !project.git ||
                        !!active.sessionId ||
                        working ||
                        !branch.trim()
                      }
                    >
                      <Plus size={14} />
                      Create worktree
                    </Button>
                  </form>
                  {sessionOpen ? (
                    <Button
                      variant="ghost"
                      className="small-button"
                      disabled={working}
                      onClick={() => void perform(() => store.close(active.id))}
                    >
                      Close session
                    </Button>
                  ) : (
                    tree &&
                    !tree.isMain && (
                      <Button
                        variant="ghost"
                        className="small-button danger-button"
                        disabled={working}
                        onClick={() =>
                          void perform(() => store.removeWorktree())
                        }
                      >
                        <Trash2 size={13} />
                        Remove clean worktree
                      </Button>
                    )
                  )}
                  <p className="panel-note">
                    Start a new chat to choose a different worktree. Removal
                    retains the branch. Git refuses worktrees with pending
                    changes.
                  </p>
                </>
              )}
            </div>
          )}
          {panel === "review" && (
            <div className="review-body">
              <div className="review-toolbar">
                <span>Compared with HEAD</span>
                <IconButton
                  label="Refresh changes"
                  disabled={working}
                  onClick={() => void perform(() => store.review())}
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
                      {state.review.diff.split("\n").map((line, i) => (
                        <div
                          key={i}
                          className={
                            line.startsWith("+") && !line.startsWith("+++")
                              ? "addition"
                              : line.startsWith("-") && !line.startsWith("---")
                                ? "deletion"
                                : line.startsWith("@@")
                                  ? "hunk"
                                  : ""
                          }
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
                      This review is truncated. Inspect the full changes with
                      Git.
                    </p>
                  )}
                  <div className="review-footer">
                    <PatchCopy text={state.review.diff} />
                  </div>
                </>
              ) : (
                <p className="panel-note">
                  {working
                    ? "Reading changes…"
                    : "Open a project and refresh its changes."}
                </p>
              )}
            </div>
          )}
        </aside>
      )}
    </div>
  );
}
