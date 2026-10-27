import type { Snapshot, StateReference } from "@swarmie/client-ui";
import { bindState } from "@swarmie/client-ui";
import { appStateOrb } from "@swarmie/plugin-app-state-browser";
import type { ShellUiApi } from "@swarmie/plugin-application-shell-browser";
import {
  applicationCommandsOrb,
  applicationShellOrb,
  createNavigation,
} from "@swarmie/plugin-application-shell-browser";
import type { Connector, Node } from "@swarmie/plugin-connections-browser";
import {
  Connections,
  connectionsOrb,
  connectWebSocket,
  ConnectionPhase,
  ConnectionFailureKind,
  unconfiguredNode,
} from "@swarmie/plugin-connections-browser";
import type { Conversation, ConversationsUiApi } from "@swarmie/plugin-conversations-browser";
import {
  conversationsOrb,
  createConversationRecords,
  defaultConversationTitle,
} from "@swarmie/plugin-conversations-browser";
import type { Notification } from "@swarmie/plugin-notifications-browser";
import { notificationsOrb } from "@swarmie/plugin-notifications-browser";
import { onboardingOrb } from "@swarmie/plugin-onboarding-browser";
import type { ProjectsPersistence } from "@swarmie/plugin-projects-browser";
import { createProjectsApi, projectsOrb } from "@swarmie/plugin-projects-browser";
import { projectsIntegrationOrb } from "@swarmie/plugin-projects-browser/integration";
import type { ProjectsPresentation } from "@swarmie/plugin-projects-browser/views";
import type { ProvidersBackend } from "@swarmie/plugin-providers-browser";
import { createProvidersApi, providersOrb } from "@swarmie/plugin-providers-browser";
import { readAloudOrb } from "@swarmie/plugin-read-aloud-browser";
import { settingsOrb } from "@swarmie/plugin-settings-browser";
import type { Tab } from "@swarmie/plugin-views-browser";
import { createViewsApi, viewsOrb } from "@swarmie/plugin-views-browser";
import type { ReferenceBase } from "@syntropika/tembra-sdk";
import type { Host } from "@syntropika/tembra-sdk/runtime";

import { createPluginControls } from "./plugins";
import { layoutSnapshot, selectSnapshot } from "./projections";
import { loadProjects, persistProjects } from "./projects";
import { createSubmissions } from "./submissions";
import type { ExecutionFactory } from "./submissions";

export type ClientOptions = {
  dev: boolean;
  connector?: Connector;
  nodes?: Node[];
  execution?: ExecutionFactory;
  providers?: ProvidersBackend;
  projectsPersistence?: ProjectsPersistence;
};

/** Mounts local capabilities without waiting for node networking. */
export async function initializeClient(host: Host, options: ClientOptions) {
  let closing = false;
  const held: ReferenceBase[] = [];
  const detachSources: (() => void)[] = [];
  let sequence = 0;
  const nextId = () => {
    sequence += 1;
    return `client-${Date.now().toString(36)}-${sequence}`;
  };
  async function stateOf<T>(pending: Promise<ReferenceBase<T> & StateReference<T>>) {
    const reference = await pending;
    held.push(reference);
    return {
      ...bindState(reference, (failure) => {
        if (!closing) {
          console.error("Presentation update failed", failure);
        }
      }),
      commit: reference.update,
    };
  }
  const savedProjects = loadProjects(options.projectsPersistence);
  const shellDefinition = applicationShellOrb(
    globalThis.matchMedia?.("(min-width: 768px)").matches ?? true,
  );
  const projectsDefinition = projectsOrb(savedProjects.data);
  await host.install([
    shellDefinition,
    conversationsOrb,
    projectsDefinition,
    connectionsOrb,
    viewsOrb,
    providersOrb,
    settingsOrb,
    notificationsOrb,
    onboardingOrb,
    ...(options.dev ? [appStateOrb] : []),
  ]);
  const conversationsReference = host.get(
    conversationsOrb.services["swarmie.conversations.ui"],
    "main",
  );
  const shellReference = host.get(shellDefinition.services["swarmie.shell.ui"], "main");
  const [
    conversationState,
    connectionState,
    providersState,
    settings,
    onboarding,
    shellState,
    views,
    projectsState,
  ] = await Promise.all([
    stateOf(conversationsReference),
    stateOf(host.get(connectionsOrb.services["swarmie.connections.ui"], "main")),
    stateOf(host.get(providersOrb.services["swarmie.providers.ui"], "main")),
    stateOf(host.get(settingsOrb.services["swarmie.settings.ui"], "main")),
    stateOf(host.get(onboardingOrb.services["swarmie.onboarding.ui"], "main")),
    stateOf(shellReference),
    stateOf(host.get(viewsOrb.services["swarmie.views.ui"], "main")),
    stateOf(host.get(projectsDefinition.services["swarmie.projects.ui"], "main")),
  ]);
  const providers = createProvidersApi(providersState, providersState.commit, options.providers);
  detachSources.push(providers.dispose);
  const conversationsOwner = await conversationsReference;
  const shellOwner = await shellReference;
  const conversationsUi = conversationsOwner.opaque<ConversationsUiApi>("ui");
  const shellUi = shellOwner.opaque<ShellUiApi>("ui");
  const records = createConversationRecords(conversationState, conversationState.commit);
  const { roster } = records;
  const { navigate, travel } = createNavigation(shellState);
  const viewCommands = createViewsApi(views, nextId);
  const projects = createProjectsApi(projectsState, projectsState.commit, nextId);
  // A failed load must not overwrite saved data with an empty projection.
  if (savedProjects.error !== null) {
    projects.fail(new Error(savedProjects.error));
  } else if (options.projectsPersistence) {
    detachSources.push(persistProjects(projectsState, options.projectsPersistence, projects.fail));
  }
  const notificationReference = await host.get(
    notificationsOrb.services["swarmie.notifications.ui"],
    "main",
  );
  held.push(notificationReference);
  const notifications: Snapshot<readonly Notification[]> = {
    getSnapshot: () => notificationReference.state.value,
    subscribe: (changed) => notificationReference.state.subscribe(changed),
  };
  const shell = layoutSnapshot(shellState, views);
  const conversationError = selectSnapshot(conversationState, (value) => value.error);
  const navigationHistory = selectSnapshot(shellState, (value) => value.history);
  const connection = selectSnapshot(
    connectionState,
    (value) => value.sessions[value.selected]?.phase ?? ConnectionPhase.Unconfigured,
  );
  const nodeConnections = new Connections(options.connector ?? connectWebSocket, (state) => {
    if (closing) {
      return;
    }
    connectionState.update((value) => ({
      ...value,
      sessions: { ...value.sessions, [state.nodeId]: state },
    }));
    if (
      state.failure?.kind === ConnectionFailureKind.ScopeChanged ||
      state.failure?.kind === ConnectionFailureKind.Principal
    ) {
      conversationState.update((value) => ({
        ...value,
        records: value.records.map((record) =>
          record.node === state.nodeId
            ? {
                ...record,
                messages: [],
                execution: { ...record.execution, phase: "unknown", label: "Node access changed." },
              }
            : record,
        ),
      }));
    }
  });
  const submissions = createSubmissions(
    nodeConnections,
    connectionState,
    records,
    providers,
    options.execution,
    conversationsUi.lifecycle,
    nextId,
  );
  let firstConversation: Conversation | undefined;
  function activeConversation(): Conversation {
    const current = roster.getSnapshot();
    const selected = current.find((chat) => chat.id === shellState.getSnapshot().active);
    const fallback = selected ?? current.find((chat) => !chat.archived) ?? firstConversation;
    if (!fallback) {
      throw new Error("The conversation presentation has not started.");
    }
    return fallback;
  }
  function openConversation(id: string) {
    navigate({ active: id, section: "conversations" });
    views.update((current) => ({ ...current, conversationVisible: true, mobile: "conversation" }));
  }
  function createConversation(node = connectionState.getSnapshot().selected): Conversation {
    const selection = providers.defaultSelection();
    const chat = records.create({
      id: nextId(),
      title: defaultConversationTitle,
      started: false,
      node: null,
      pinned: false,
      archived: false,
      draft: {
        text: "",
        attachments: [],
        account: selection?.account ?? null,
        model: selection?.model ?? "",
        effort: selection?.effort ?? "",
        node,
        permission: "Ask",
      },
      messages: [],
      execution: { phase: "idle", label: "", queue: [], steering: [] },
    });
    firstConversation ??= chat;
    openConversation(chat.id);
    return chat;
  }
  function renameConversation(chat: Conversation, title: string) {
    records.update(chat.id, (record) => ({ ...record, title }));
  }
  function togglePin(chat: Conversation) {
    records.update(chat.id, (record) => ({ ...record, pinned: !record.pinned }));
  }
  function archive(chat: Conversation) {
    records.update(chat.id, (record) => ({ ...record, archived: true }));
    if (shellState.getSnapshot().active === chat.id) {
      const next = roster.getSnapshot().find((item) => item.id !== chat.id && !item.archived);
      if (next) {
        openConversation(next.id);
      } else {
        createConversation();
      }
    }
  }
  function openTab(kind: Tab["kind"] = "start") {
    navigate({ section: "conversations" });
    viewCommands.open(kind, activeConversation());
  }
  const projectsPresentation: ProjectsPresentation = {
    api: projects,
    conversations: roster,
    nodes: selectSnapshot(connectionState, (value) => value.nodes),
    open: (projectId) => {
      navigate({ section: "projects", params: projectId === undefined ? {} : { projectId } });
      hideMobileNavigation();
    },
    openConversation: (id) => {
      openConversation(id);
      hideMobileNavigation();
    },
    createConversation: async (projectId) => {
      const chat = createConversation();
      await projects.stageConversation(chat.id, projectId);
    },
  };
  function hideMobileNavigation() {
    if (globalThis.matchMedia?.("(max-width: 767px)").matches) {
      shellState.update((value) => ({ ...value, navigation: false }));
    }
  }
  const speechDefinition = readAloudOrb(() => settings.getSnapshot().language);
  const projectsIntegrationDefinition = projectsIntegrationOrb(projects, {
    projects: projectsDefinition.services["swarmie.projects.ui"],
    shell: shellDefinition.services["swarmie.shell.ui"],
  });
  const controls = createPluginControls(
    settings,
    options.dev,
    {
      projects: async () =>
        await host.get(
          projectsIntegrationDefinition.services["swarmie.projects.integration"],
          "main",
        ),
      "read-aloud": async () =>
        await host.get(speechDefinition.services["swarmie.read-aloud.ui"], "main"),
      "app-state": async () => await host.get(appStateOrb.services["swarmie.app-state.ui"], "main"),
    },
    (id) => {
      if (
        shellUi.sections
          .getSnapshot()
          .some(
            (section) => section.id === shellState.getSnapshot().section && section.owner === id,
          )
      ) {
        navigate({ section: "conversations" });
      }
    },
    records.fail,
  );
  await host.install(speechDefinition);
  await controls.activate("read-aloud");
  if (options.dev) {
    await controls.activate("app-state");
  }
  const commandsDefinition = applicationCommandsOrb({
    openView: () => {
      openTab();
    },
    showContext: () => {
      shellState.update((current) => ({ ...current, summary: true }));
    },
    stop: () => {
      submissions.stop(activeConversation());
    },
  });
  await host.install(commandsDefinition);
  held.push(await host.get(commandsDefinition.services["swarmie.shell.commands"], "main"));
  await host.install(projectsIntegrationDefinition);
  await controls.activate("projects");
  const initialNodes = options.nodes ?? [unconfiguredNode];
  connectionState.update((value) => ({
    ...value,
    nodes: initialNodes,
    selected: initialNodes[0]?.id ?? "",
  }));
  for (const node of initialNodes) {
    nodeConnections.configure(node);
  }
  const [initialNode] = initialNodes;
  if (initialNode && (initialNode.url !== null || options.connector)) {
    nodeConnections.open(initialNode.id).catch(() => undefined);
  }
  createConversation(initialNode?.id);
  detachSources.push(
    providers.state.subscribe(() => {
      const selection = providers.defaultSelection();
      if (
        selection === null ||
        !conversationState
          .getSnapshot()
          .records.some((record) => !record.archived && record.draft.account === null)
      ) {
        return;
      }
      conversationState.update((current) => ({
        ...current,
        records: current.records.map((record) =>
          record.archived || record.draft.account !== null
            ? record
            : { ...record, draft: { ...record.draft, ...selection } },
        ),
      }));
    }),
  );
  function disconnect(nodeId = connectionState.getSnapshot().selected) {
    nodeConnections.disconnect(nodeId);
  }
  async function connect(nodeId = connectionState.getSnapshot().selected) {
    return await nodeConnections.open(nodeId);
  }
  function selectNode(nodeId: string) {
    if (!connectionState.getSnapshot().nodes.some((node) => node.id === nodeId)) {
      throw new Error("The selected node is not configured.");
    }
    connectionState.update((value) => ({ ...value, selected: nodeId }));
  }
  async function configureNode(node: Node) {
    nodeConnections.configure(node);
    connectionState.update((value) => ({
      ...value,
      nodes: [
        ...value.nodes.filter(
          (entry) =>
            entry.id !== node.id && !(entry.id === unconfiguredNode.id && entry.url === null),
        ),
        node,
      ],
      selected: node.id,
    }));
    return await nodeConnections.open(node.id);
  }
  function diagnostics() {
    return {
      installedOrbs: host.installedOrbs().map((entry) => entry.name),
      kernel: host.diagnostics(),
    };
  }
  async function close() {
    closing = true;
    for (const detach of detachSources) {
      detach();
    }
    nodeConnections.close();
    controls.close();
    for (const reference of held) {
      reference.release();
    }
    await host.close();
  }
  return {
    conversationsUi,
    shellUi,
    projects,
    projectsState,
    projectsPresentation,
    conversationState,
    conversationError,
    connectionState,
    providers,
    settings,
    onboarding,
    shellState,
    views,
    connection,
    notifications,
    plugins: controls.plugins,
    shell,
    navigationHistory,
    roster,
    nextId,
    activeConversation,
    createConversation,
    navigate,
    travel,
    openConversation,
    renameConversation,
    togglePin,
    archive,
    openTab,
    closeTab: viewCommands.close,
    showConversation: viewCommands.showConversation,
    toggleConversation: viewCommands.toggleConversation,
    expandView: () => {
      viewCommands.expand(activeConversation());
    },
    disconnect,
    connect,
    configureNode,
    selectNode,
    nodeConnections,
    togglePlugin: controls.togglePlugin,
    ...submissions,
    diagnostics,
    close,
  };
}
export type ClientUi = Awaited<ReturnType<typeof initializeClient>>;
