import type { StateReference } from "@swarmie/client-ui";
import { bindState } from "@swarmie/client-ui";
import { createActivityApi, activityOrb } from "@swarmie/plugin-activity-browser";
import { appStateOrb } from "@swarmie/plugin-app-state-browser";
import type { Connector, Node } from "@swarmie/plugin-browser/connections";
import {
  Connections,
  connectionsOrb,
  connectWebSocket,
  ConnectionPhase,
  ConnectionFailureKind,
  unconfiguredNode,
} from "@swarmie/plugin-browser/connections";
import { onboardingOrb } from "@swarmie/plugin-browser/onboarding";
import { loadToolDetails, settingsOrb } from "@swarmie/plugin-browser/settings";
import type { ToolDetailsPersistence } from "@swarmie/plugin-browser/settings";
import type { ShellUiApi } from "@swarmie/plugin-browser/shell";
import {
  applicationCommandsOrb,
  applicationShellOrb,
  createNavigation,
} from "@swarmie/plugin-browser/shell";
import type { BuiltinTab, CapabilityView } from "@swarmie/plugin-browser/views";
import { createViewsApi, viewsOrb } from "@swarmie/plugin-browser/views";
import { contextUiOrb, createContextPresentation } from "@swarmie/plugin-context-browser";
import type { Conversation, ConversationsUiApi } from "@swarmie/plugin-conversations-browser";
import {
  conversationsOrb,
  createConversationRecords,
  defaultConversationTitle,
} from "@swarmie/plugin-conversations-browser";
import {
  createGitPresentation,
  gitBrowserOrb,
  gitIntegrationOrb,
} from "@swarmie/plugin-git-browser";
import { mcpOrb, createMcpApi } from "@swarmie/plugin-mcp-browser";
import type { ProjectsBackend } from "@swarmie/plugin-projects-browser";
import {
  createNativeProjects,
  createProjectsApi,
  projectsOrb,
} from "@swarmie/plugin-projects-browser";
import { projectsIntegrationOrb } from "@swarmie/plugin-projects-browser/integration";
import type { ProjectsPresentation } from "@swarmie/plugin-projects-browser/views";
import type { ProvidersBackend } from "@swarmie/plugin-providers-browser";
import { providersOrb } from "@swarmie/plugin-providers-browser";
import { readAloudOrb } from "@swarmie/plugin-read-aloud-browser";
import { createVaultUi, vaultUiOrb } from "@swarmie/plugin-vault-browser";
import type { VaultBackend } from "@swarmie/plugin-vault-browser";
import type { ReferenceBase } from "@syntropika/tembra-sdk";
import type { Host } from "@syntropika/tembra-sdk/runtime";

import { contextSelection } from "./context-selection";
import { createDirectoryBrowser } from "./directories";
import { admitGitContext } from "./git-admission";
import { createNodeExecution } from "./node-execution";
import { observeOnboarding } from "./onboarding";
import { createPluginControls } from "./plugins";
import { layoutSnapshot, selectSnapshot } from "./projections";
import { createProvidersPresentation } from "./provider-node";
import { createSubmissions } from "./submissions";
import type { ExecutionFactory } from "./submissions";

export type ClientOptions = {
  dev: boolean;
  nodeServices?: boolean;
  connector?: Connector;
  nodes?: Node[];
  execution?: ExecutionFactory;
  providers?: ProvidersBackend;
  projects?: ProjectsBackend;
  browserProjectsProfileKey?: string;
  toolDetailsPersistence?: ToolDetailsPersistence;
  vault?: VaultBackend;
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
  const shellDefinition = applicationShellOrb(
    globalThis.matchMedia?.("(min-width: 768px)").matches ?? true,
  );
  const projectsDefinition = projectsOrb();
  await host.install([
    shellDefinition,
    conversationsOrb,
    contextUiOrb,
    projectsDefinition,
    connectionsOrb,
    viewsOrb,
    providersOrb,
    settingsOrb,
    mcpOrb,
    gitBrowserOrb,
    activityOrb,
    vaultUiOrb,
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
    mcpState,
    gitState,
  ] = await Promise.all([
    stateOf(conversationsReference),
    stateOf(host.get(connectionsOrb.services["swarmie.connections.ui"], "main")),
    stateOf(host.get(providersOrb.services["swarmie.providers.ui"], "main")),
    stateOf(host.get(settingsOrb.services["swarmie.settings.ui"], "main")),
    stateOf(host.get(onboardingOrb.services["swarmie.onboarding.ui"], "main")),
    stateOf(shellReference),
    stateOf(host.get(viewsOrb.services["swarmie.views.ui"], "main")),
    stateOf(host.get(projectsDefinition.services["swarmie.projects.ui"], "main")),
    stateOf(host.get(mcpOrb.services["swarmie.mcp.ui"], "main")),
    stateOf(host.get(gitBrowserOrb.services["swarmie.git.ui"], "main")),
  ]);
  await settings.commit((current) => ({
    ...current,
    toolDetails: loadToolDetails(options.toolDetailsPersistence),
  }));
  if (options.toolDetailsPersistence) {
    const persistence = options.toolDetailsPersistence;
    let previous = settings.getSnapshot().toolDetails;
    detachSources.push(
      settings.subscribe(() => {
        const current = settings.getSnapshot().toolDetails;
        if (current === previous) {
          return;
        }
        previous = current;
        try {
          persistence.save(current);
        } catch {
          // A blocked store does not prevent this session's display choice.
          console.warn("The tool display preference could not be saved.");
        }
      }),
    );
  }
  const vaultState = await stateOf(host.get(vaultUiOrb.services["swarmie.vault.ui"], "main"));
  const contextState = await stateOf(host.get(contextUiOrb.services["swarmie.context.ui"], "main"));
  const vault = createVaultUi(
    vaultState,
    (value) => {
      vaultState.set(value);
    },
    options.vault,
  );
  detachSources.push(() => {
    vault.close();
  });
  const conversationsOwner = await conversationsReference;
  const shellOwner = await shellReference;
  const conversationsUi = conversationsOwner.opaque<ConversationsUiApi>("ui");
  const shellUi = shellOwner.opaque<ShellUiApi>("ui");
  const records = createConversationRecords(conversationState, conversationState.commit);
  const { roster } = records;
  const { navigate, travel } = createNavigation(shellState);
  const viewCommands = createViewsApi(views, nextId);
  const activityState = await stateOf(
    host.get(activityOrb.services["swarmie.activity.ui"], "main"),
  );
  const activity = selectSnapshot(activityState, (value) => value.items);
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
                execution: {
                  ...record.execution,
                  phase: "unknown",
                  label: "Node access changed.",
                  canAcknowledge: false,
                },
              }
            : record,
        ),
      }));
    }
  });
  const nativeProjects = createNativeProjects(
    projectsState,
    connectionState,
    nodeConnections,
    options.projects,
  );
  const projects = createProjectsApi(projectsState, nativeProjects.backend, () =>
    crypto.randomUUID(),
  );
  const activityApi = createActivityApi(activityState, connectionState, nodeConnections);
  detachSources.push(() => {
    nativeProjects.close();
  }, activityApi.close);
  const gitPresentation = createGitPresentation(gitState, connectionState, nodeConnections);
  const gitIntegrationDefinition = gitIntegrationOrb(gitPresentation);
  await host.install(gitIntegrationDefinition);
  const providerPresentation = createProvidersPresentation(
    providersState,
    providersState.commit,
    connectionState,
    nodeConnections,
    options.providers,
    options.nodeServices,
  );
  const providers = providerPresentation.api;
  detachSources.push(
    () => {
      providerPresentation.dispose();
    },
    observeOnboarding(onboarding, providers.state),
  );
  const mcp = createMcpApi(mcpState, connectionState, nodeConnections);
  detachSources.push(() => {
    mcp.dispose();
  });
  const nodeExecution =
    options.nodeServices === true && options.execution === undefined
      ? createNodeExecution(
          connectionState,
          nodeConnections,
          records,
          () => shellState.getSnapshot().active,
        )
      : null;
  if (nodeExecution !== null) {
    detachSources.push(() => {
      nodeExecution.dispose();
    });
  }
  const submissions = createSubmissions(
    nodeConnections,
    connectionState,
    records,
    providers,
    options.execution ?? nodeExecution?.factory,
    conversationsUi.lifecycle,
    nextId,
    nodeExecution,
    admitGitContext,
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
    viewCommands.showConversation();
    const chat = roster.getSnapshot().find((entry) => entry.id === id);
    if (nodeExecution !== null && chat?.node !== null && chat?.node !== undefined) {
      nodeExecution.observe(chat.node, id).catch(records.fail);
    }
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
      },
      messages: [],
      execution: { phase: "idle", label: "", queue: [], steering: [] },
    });
    firstConversation ??= chat;
    openConversation(chat.id);
    return chat;
  }
  function newSideChat(chat: Conversation): Conversation {
    viewCommands.showConversation();
    viewCommands.open("conversation", chat);
    return createConversation();
  }
  function changeMetadata(
    chat: Conversation,
    change: (
      record: Pick<Conversation, "title" | "pinned" | "archived">,
    ) => Partial<Pick<Conversation, "title" | "pinned" | "archived">>,
  ) {
    if (chat.node !== null && nodeExecution !== null) {
      nodeExecution.metadata(chat.node, chat.id, change).catch(records.fail);
    } else {
      records.commit(chat.id, (record) => ({ ...record, ...change(record) })).catch(records.fail);
    }
  }
  function renameConversation(chat: Conversation, title: string) {
    changeMetadata(chat, () => ({ title }));
  }
  function togglePin(chat: Conversation) {
    changeMetadata(chat, (record) => ({ pinned: !record.pinned }));
  }
  function archive(chat: Conversation) {
    changeMetadata(chat, () => ({ archived: true }));
    if (shellState.getSnapshot().active === chat.id) {
      const next = roster.getSnapshot().find((item) => item.id !== chat.id && !item.archived);
      if (next) {
        openConversation(next.id);
      } else {
        createConversation();
      }
    }
  }
  function openTab(kind: BuiltinTab["kind"] = "start") {
    navigate({ section: "conversations" });
    viewCommands.open(kind, activeConversation());
  }
  function openCapabilityTab(descriptor: CapabilityView & { title: string }) {
    navigate({ section: "conversations" });
    viewCommands.openCapability(descriptor);
  }
  const projectsPresentation: ProjectsPresentation = {
    api: projects,
    ...(options.browserProjectsProfileKey === undefined
      ? {}
      : { browserProfileKey: options.browserProjectsProfileKey }),
    directories: createDirectoryBrowser(nodeConnections),
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
      git: async () =>
        await host.get(gitIntegrationDefinition.services["swarmie.git.integration"], "main"),
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
  await controls.activate("git");
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
  const context = createContextPresentation(
    contextState,
    connectionState,
    nodeConnections,
    contextSelection(activeConversation, [shellState, roster]),
    conversationsUi.composer,
  );
  detachSources.push(
    () => {
      context.close();
    },
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
    vault,
    context,
    conversationsUi,
    shellUi,
    projects,
    projectsState,
    projectsPresentation,
    conversationState,
    conversationError,
    connectionState,
    providers,
    mcp,
    gitPresentation,
    settings,
    onboarding,
    shellState,
    views,
    connection,
    activity,
    activityApi,
    plugins: controls.plugins,
    shell,
    navigationHistory,
    roster,
    nextId,
    activeConversation,
    createConversation,
    newSideChat,
    navigate,
    travel,
    openConversation,
    renameConversation,
    togglePin,
    archive,
    openTab,
    openCapabilityTab,
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
