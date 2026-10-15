import { useMutation } from "@tanstack/react-query";
import { Monitor } from "lucide-react";
// Copyright 2026 Syntropika
// SPDX-License-Identifier: Apache-2.0
import { useEffect, useRef, useState, useSyncExternalStore } from "react";

import { startAction } from "~/src/action-boundary";
import { sortedCopy } from "~/src/collection-order";
import { Select } from "~/src/components/ui/select";
import { ContextUsage } from "~/src/context-usage";
import type { ClientContribution } from "~/src/contribution-host";
import type { ContributionContext } from "~/src/contributions";
import type { TranscriptionProvider } from "~/src/dictation";
import { HarnessSelect } from "~/src/harness-select";
import { completeHarnessSetup, defaultHarness, readHarnessSetup } from "~/src/harness-setup";
import type { NodeClient, NodesState } from "~/src/nodes";
import { hasConversation } from "~/src/protocol";
import type { Thread } from "~/src/protocol";
import { SessionControls } from "~/src/session-controls";
import { readShortcuts, matchesShortcut } from "~/src/shortcuts";
import { readSidebarSorting, sortThreads, projectActivity } from "~/src/sidebar-preferences";
import type { SidebarSorting } from "~/src/sidebar-preferences";
import { watchThreadTimes } from "~/src/thread-clock";
import { hasText, hasNumber } from "~/src/value-presence";
import type { WorkspaceStore } from "~/src/workspace";

export type Panel = "project" | "settings" | "review" | "extensions" | "client" | null;

export const blank: Thread = {
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

export type NodeWorkspaceProps = {
  store: WorkspaceStore;
  nodes: NodesState;
  client: NodeClient;
  clientContributions?: readonly ClientContribution[];
  transcriptionProvider?: TranscriptionProvider;
};

export function useWorkspaceState({
  store,
  nodes,
  client,
  clientContributions,
  transcriptionProvider,
}: NodeWorkspaceProps) {
  const state = useSyncExternalStore(store.subscribe, store.getSnapshot);
  const [setupOpen, setSetupOpen] = useState<boolean | null>(null);
  const finishSetup = () => {
    const harness = defaultHarness(state.harnesses, state.node);
    if (state.node) {
      try {
        completeHarnessSetup(state.node, harness);
      } catch {
        store.setError(
          "Could not save setup preferences. You can continue and try again in Settings.",
        );
      }
    }
    const draft = store.active();
    if (
      harness &&
      draft &&
      !hasText(draft.sessionId) &&
      !draft.projectId &&
      !hasConversation(draft) &&
      !hasText(draft.draft) &&
      !hasNumber(draft.attachments?.length) &&
      !hasNumber(draft.contextItems?.length) &&
      !draft.model &&
      !Object.keys(draft.config ?? {}).length
    ) {
      store.updateThread(draft.id, {
        harness,
        model: "",
        config: {},
        configurations: undefined,
        models: undefined,
        actualModel: undefined,
      });
    }
    setSetupOpen(false);
  };
  const showSetup =
    setupOpen ??
    (state.connected && state.node !== undefined && !readHarnessSetup(state.node).completed);
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
  const [extensionScreen, setExtensionScreen] = useState<string | undefined>(readExtensionRoute);
  const [surfaceContext, setSurfaceContext] = useState<ContributionContext>();
  const [extensionPanel, setExtensionPanel] = useState<string>();
  const closeScreen = () => {
    setExtensionScreen(undefined);
    if (window.location.hash.startsWith("#/extension/")) {
      window.history.replaceState(null, "", window.location.pathname + window.location.search);
    }
  };
  useEffect(() => {
    const route = () => {
      setExtensionScreen(readExtensionRoute());
    };
    window.addEventListener("hashchange", route);
    return () => {
      window.removeEventListener("hashchange", route);
    };
  }, []);

  const [settingsSection, setSettingsSection] = useState<"general" | "account" | "security">(
    "general",
  );
  const [palette, setPalette] = useState(false);
  const [finder, setFinder] = useState(false);
  const [shortcuts, setShortcuts] = useState(readShortcuts);
  const [organization, setOrganization] = useState<"projects" | "time" | "working">(() => {
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
    run: () => Promise<unknown> | undefined;
  }>();
  const [archived, setArchived] = useState(false);
  const [renaming, setRenaming] = useState(false);
  const [title, setTitle] = useState("");
  const [query, setQuery] = useState("");
  const [search, setSearch] = useState(false);
  const [projectDialog, setProjectDialog] = useState(false);
  const [branch, setBranch] = useState("");
  const actionMutation = useMutation({
    mutationFn: async (action: () => Promise<unknown>) => await action(),
    retry: false,
    gcTime: 0,
  });
  const working = actionMutation.isPending;
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [sidebarSort, setSidebarSort] = useState<SidebarSorting>(readSidebarSorting);
  useEffect(() => {
    try {
      localStorage.setItem("swarmie.sidebar-sorting", JSON.stringify(sidebarSort));
    } catch (error) {
      store.setError(error);
    }
  }, [sidebarSort, store]);
  const [navigation, setNavigation] = useState<{ ids: string[]; index: number }>({
    ids: [],
    index: -1,
  });
  const active = state.threads.find((t) => t.id === state.activeId) ?? blank;
  const surfaceScope = `${active.id}:${active.projectId}:${active.worktreeId}:${active.sessionId}`;
  const [observedSurfaceScope, setObservedSurfaceScope] = useState(surfaceScope);
  if (observedSurfaceScope !== surfaceScope) {
    setObservedSurfaceScope(surfaceScope);
    setExtensionScreen(undefined);
    setExtensionPanel(undefined);
    setSurfaceContext(undefined);
    if (panel === "client") {
      setPanel(null);
    }
  }
  const routeScope = useRef(surfaceScope);
  useEffect(() => {
    if (routeScope.current !== surfaceScope && window.location.hash.startsWith("#/extension/")) {
      window.history.replaceState(null, "", window.location.pathname + window.location.search);
    }
    routeScope.current = surfaceScope;
  }, [surfaceScope]);
  const project = state.projects.find((p) => p.id === active.projectId);
  const tree = project?.worktrees.find((t) => t.id === active.worktreeId);
  const sessionOpen = hasText(active.sessionId) && !["closed", "failed"].includes(active.state);
  const ready =
    state.connected &&
    !!active.worktreeId &&
    state.harnesses.some((h) => h.harness === active.harness && h.available) &&
    (hasText(active.sessionId) || !state.disabledHarnesses.includes(active.harness));
  const filtered = state.threads.filter(
    (t) =>
      hasConversation(t) &&
      (t.archived === true) === archived &&
      (showSnoozed ||
        (!(t.settled === true) && (!hasNumber(t.snoozeUntil) || t.snoozeUntil <= now))) &&
      (t.title.toLowerCase().includes(query.toLowerCase()) ||
        t.messages.some((m) =>
          typeof m.content === "string"
            ? m.content.toLowerCase().includes(query.toLowerCase())
            : m.content.some(
                (p) => p.type === "text" && p.text.toLowerCase().includes(query.toLowerCase()),
              ),
        )),
  );
  const pinned = sortThreads(
    filtered.filter((t) => t.pinned),
    sidebarSort.pinned,
  );
  const projects =
    sidebarSort.projects === "manual"
      ? state.projects
      : sortedCopy(
          state.projects,
          (a, b) => projectActivity(state.threads, b.id) - projectActivity(state.threads, a.id),
        );
  useEffect(() => watchThreadTimes(state.threads, setNow), [state.threads]);
  useEffect(() => {
    try {
      localStorage.setItem("swarmie.sidebar-width", String(sidebarWidth));
    } catch (error) {
      store.setError(error);
    }
  }, [sidebarWidth, store]);
  useEffect(() => {
    try {
      localStorage.setItem("swarmie.organization", organization);
    } catch (error) {
      store.setError(error);
    }
  }, [organization, store]);
  const navigationApplied = useRef(false);
  useEffect(() => {
    async function applyNavigation() {
      const searchParams = new URLSearchParams(window.location.search);
      const initialProject = searchParams.get("project");
      const initialSession = searchParams.get("session");
      if (hasText(initialProject) && !(await store.openProject(initialProject))) {
        return;
      }
      const thread = store.getSnapshot().threads.find((t) => t.sessionId === initialSession);
      if (thread) {
        store.select(thread.id);
      }
    }
    if (
      state.connected &&
      !navigationApplied.current &&
      nodes.nodes.find((node) => node.identity.environmentId === state.node?.environmentId)
        ?.local === true
    ) {
      navigationApplied.current = true;
      startAction(applyNavigation(), store.setError.bind(store));
    }
  }, [state.connected, state.node, nodes.nodes, store]);
  useEffect(() => {
    const compact = window.matchMedia("(max-width: 760px)");
    const adapt = () => {
      if (compact.matches) {
        setSidebar(false);
      }
    };
    compact.addEventListener("change", adapt);
    return () => {
      compact.removeEventListener("change", adapt);
    };
  }, []);
  useEffect(() => {
    const opened = () => {
      setPanel("extensions");
    };
    window.addEventListener("swarmie:open-tools", opened);
    return () => {
      window.removeEventListener("swarmie:open-tools", opened);
    };
  }, []);
  const [panelOwner, setPanelOwner] = useState(active.id);
  if (panelOwner !== active.id) {
    setPanelOwner(active.id);
    setPanel((current) => (current === "settings" ? current : savedPanel(active.layout?.panel)));
  }
  useEffect(() => {
    if (active !== blank && active.layout?.panel !== panel) {
      store.updateThread(active.id, { layout: { ...active.layout, panel } }, false);
    }
  }, [panel, active, store]);
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.defaultPrevented) {
        return;
      }
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
        if (window.innerWidth <= 760) {
          setSidebar(false);
        }
        setPalette(false);
        setFinder(false);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
    };
  }, [shortcuts, store]);
  const perform = async (action: () => Promise<unknown>) => {
    try {
      await actionMutation.mutateAsync(action);
    } catch (error) {
      store.setError(error);
    }
  };
  const select = (id: string) => {
    closeScreen();
    setNavigation((previous) => {
      const ids = [...previous.ids.slice(0, previous.index + 1), id];
      return { ids, index: ids.length - 1 };
    });
    store.select(id);
    if (window.innerWidth < 760) {
      setSidebar(false);
    }
  };
  const navigate = (direction: number) => {
    closeScreen();
    const index = navigation.index + direction;
    if (index >= 0 && index < navigation.ids.length) {
      setNavigation({ ...navigation, index });
      store.select(navigation.ids[index]);
    }
  };
  const newChat = (projectId?: string) => {
    closeScreen();
    store.createThread(projectId);
    setPanel(null);
    if (window.innerWidth < 760) {
      setSidebar(false);
    }
  };
  const renameActive = () => {
    setTitle(active.title);
    setRenaming(true);
  };
  const archiveDisabled =
    active === blank || ["running", "needs_input", "starting"].includes(active.state);
  const archiveActive = () => {
    const { id } = active;
    const previous = active.archived === true;
    startAction(
      perform(async () => {
        await store.archive(id, !previous);
        setUndo({
          title: previous ? "Conversation restored" : "Conversation archived",
          run: async () => {
            await store.archive(id, previous);
          },
        });
      }),
      store.setError.bind(store),
    );
  };
  const pinActive = () => {
    store.updateThread(active.id, { pinned: !active.pinned });
  };

  const toggleProject = (id: string) => {
    setCollapsed((previous) => {
      const next = new Set(previous);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      return next;
    });
  };
  const composerContext = (
    <>
      <HarnessSelect
        label="Agent tool"
        appearance="menu"

        status={{ state: active.state, connected: state.connected }}
        disabled={hasText(active.sessionId) || !state.connected}
        value={active.harness}
        onChange={(harness) => {
          store.updateThread(active.id, {
            harness,
            model: "",
            config: {},
            configurations: undefined,
            models: undefined,
            actualModel: undefined,
          });
        }}
        harnesses={state.harnesses}
        disabledHarnesses={state.disabledHarnesses}
      />
      <div className="composer-spacer" />
      <Select
        label="View node"
        variant="composer"
        className="max-w-[50%]"
        value={state.node?.environmentId ?? ""}
        icon={<Monitor size={14} />}
        options={nodes.nodes
          .filter(
            (node) => node.registered || node.identity.environmentId === state.node?.environmentId,
          )
          .map((node) => ({
            id: node.identity.environmentId,
            label: node.displayName,
            disabled: !node.registered,
          }))}
        onChange={(id) => {
          startAction(client.select(id), store.setError.bind(store));
        }}
      />
    </>
  );
  const composerControls = (
    <>
      <ContextUsage context={active.context} />
      <SessionControls thread={active} store={store} connected={state.connected} />
    </>
  );

  return {
    store,
    state,
    setSidebar,
    panel,
    setPanel,
    setPalette,
    setFinder,
    setUndo,
    renaming,
    setRenaming,
    title,
    setTitle,
    navigation,
    active,
    project,
    perform,
    navigate,
    renameActive,
    archiveDisabled,
    archiveActive,
    pinActive,
    nodes,
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
    select,
    extensionScreen,
    surfaceContext,
    closeScreen,
    undo,
    ready,
    composerContext,
    composerControls,
    extensionPanel,
    setExtensionPanel,
    branch,
    setBranch,
    working,
    tree,
    sessionOpen,
    client,
    clientContributions,
    transcriptionProvider,
    showSetup,
    setSetupOpen,
    finishSetup,
    sidebar,
    setExtensionScreen,
    setSurfaceContext,
    settingsSection,
    setSettingsSection,
    palette,
    finder,
    shortcuts,
    setShortcuts,
    setArchived,
    projectDialog,
  };
}

export type WorkspaceViewState = ReturnType<typeof useWorkspaceState>;

function savedPanel(value: string | null | undefined): Panel {
  return value === "project" || value === "settings" || value === "review" || value === "extensions"
    ? value
    : null;
}

function readExtensionRoute(): string | undefined {
  return /^#\/extension\/(?<view>[^/]+\/[^/]+)$/u.exec(window.location.hash)?.groups?.view;
}
