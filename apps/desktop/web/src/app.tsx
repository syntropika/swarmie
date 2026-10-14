import { Home, History, GitBranch, Layers, Settings } from "lucide-react";
// Copyright 2026 Syntropika
// SPDX-License-Identifier: Apache-2.0
import { useEffect, useSyncExternalStore } from "react";

import { Button } from "@/components/ui/button";

import { AccountMenu } from "./account-menu";
import { startAction } from "./action-boundary";
import { CommandPalette } from "./command-palette";
import { ContributionHost, ContributionSlot } from "./contribution-host";
import { FileFinder } from "./file-finder";
import { HarnessOnboarding } from "./harness-onboarding";
import { nodeKey } from "./node-scope";
import { ProjectDialog } from "./project-dialog";
import { NodeQueryBoundary } from "./query-boundary";
import { SettingsPage } from "./settings-page";
import { hasText, hasNumber } from "./value-presence";
import { WorkspaceHeader } from "./workspace-header";
import { IconButton } from "./workspace-icon-button";
import { nodeClient } from "./workspace-instance";
import { WorkspaceConversation, WorkspaceDetailPanel } from "./workspace-panels";
import { WorkspaceShell } from "./workspace-shell";
import { WorkspaceSidebar } from "./workspace-sidebar";
import type { NodeWorkspaceProps } from "./workspace-state";
import { useWorkspaceState } from "./workspace-state";

export function App() {
  const nodes = useSyncExternalStore(nodeClient.subscribe, nodeClient.getSnapshot);
  useEffect(() => {
    const { store } = nodeClient.getSnapshot();
    startAction(nodeClient.start(), store.setError.bind(store));
    return () => {
      nodeClient.dispose();
    };
  }, []);
  const { node } = nodes.store.getSnapshot();
  return (
    <NodeWorkspace
      key={node ? nodeKey(node) : "local-startup"}
      store={nodes.store}
      nodes={nodes}
      client={nodeClient}
    />
  );
}

export function NodeWorkspace(props: NodeWorkspaceProps) {
  const current = useSyncExternalStore(props.store.subscribe, props.store.getSnapshot);
  const scope = current.node ? nodeKey(current.node) : "local-startup";
  return (
    <NodeQueryBoundary key={scope}>
      <WorkspaceView {...props} />
    </NodeQueryBoundary>
  );
}

function WorkspaceView(props: NodeWorkspaceProps) {
  const view = useWorkspaceState(props);
  const {
    store,
    nodes,
    client,
    clientContributions,
    transcriptionProvider,
    state,
    showSetup,
    setSetupOpen,
    finishSetup,
    sidebar,
    setSidebar,
    sidebarWidth,
    panel,
    setPanel,
    extensionScreen,
    setExtensionScreen,
    setSurfaceContext,
    extensionPanel,
    setExtensionPanel,
    closeScreen,
    settingsSection,
    setSettingsSection,
    palette,
    setPalette,
    finder,
    setFinder,
    shortcuts,
    setShortcuts,
    archived,
    setArchived,
    projectDialog,
    setProjectDialog,
    active,
    project,
    ready,
    perform,
  } = view;
  return (
    <ContributionHost
      store={store}
      thread={active}
      ready={ready && !hasText(extensionScreen) && panel !== "settings"}
      panelId={panel === "client" ? extensionPanel : (panel ?? undefined)}
      extensions={state.extensions}
      clientContributions={clientContributions}
      transcriptionProvider={transcriptionProvider}
      openSurface={(kind, id, context) => {
        setSurfaceContext(context);
        if (kind === "screen") {
          setExtensionScreen(id);
          setPanel(null);
          window.location.hash = `/extension/${id}`;
        } else {
          setExtensionPanel(id);
          setPanel("client");
        }
      }}
    >
      <WorkspaceShell
        sidebar={sidebar}
        settings={panel === "settings"}
        panelWidth={active.layout?.width ?? 370}
        sidebarWidth={sidebarWidth}
      >
        <WorkspaceHeader view={view} />
        <nav className="activity-rail" aria-label="Workspace navigation">
          <IconButton
            label="Chats"
            active={!panel && !archived}
            onClick={() => {
              closeScreen();
              setPanel(null);
              setArchived(false);
              setSidebar(true);
            }}
          >
            <Home size={18} />
          </IconButton>
          <IconButton
            label="Archived chats"
            active={archived}
            onClick={() => {
              setArchived((value) => !value);
              setSidebar(true);
            }}
          >
            <History size={18} />
          </IconButton>
          <div className="rail-divider" />
          <IconButton
            label="Review changes"
            active={panel === "review"}
            disabled={!(project?.git === true)}
            onClick={() => {
              setPanel(panel === "review" ? null : "review");
              startAction(
                perform(async () => {
                  await store.review();
                }),
                store.setError.bind(store),
              );
            }}
          >
            <GitBranch size={18} />
          </IconButton>
          <IconButton
            label="Workspace tools"
            active={panel === "extensions"}
            disabled={!state.extensions.some((e) => hasNumber(e.views?.length))}
            onClick={() => {
              setPanel(panel === "extensions" ? null : "extensions");
            }}
          >
            <Layers size={18} />
          </IconButton>
          <ContributionSlot name="sidebar.primary" />
          <div className="rail-spacer" />
          <AccountMenu
            onOpen={(section) => {
              setSettingsSection(section);
              setPanel("settings");
            }}
            onNotice={(message) => {
              store.setError(message);
            }}
          />
          <IconButton
            label="Settings"
            active={panel === "settings"}
            onClick={() => {
              setSettingsSection("general");
              setPanel(panel === "settings" ? null : "settings");
            }}
          >
            <Settings size={18} />
          </IconButton>
        </nav>
        <WorkspaceSidebar view={view} />
        {sidebar && (
          <Button
            variant="backdrop"
            aria-label="Close sidebar"
            onClick={() => {
              setSidebar(false);
            }}
          />
        )}
        <WorkspaceConversation view={view} />
        {palette && (
          <CommandPalette
            store={store}
            onClose={() => {
              setPalette(false);
            }}
            onPanel={setPanel}
            onOpenProject={() => {
              setPanel(null);
              setProjectDialog(true);
            }}
          />
        )}
        {finder && (
          <FileFinder
            store={store}
            onClose={() => {
              setFinder(false);
            }}
          />
        )}
        {projectDialog && (
          <ProjectDialog
            store={store}
            onClose={() => {
              setProjectDialog(false);
            }}
          />
        )}
        {panel === "settings" && (
          <SettingsPage
            initialSection={settingsSection}
            client={client}
            nodes={nodes}
            store={store}
            state={state}
            shortcuts={shortcuts}
            setShortcuts={setShortcuts}
            onClose={() => {
              setPanel(null);
            }}
            onSetup={() => {
              setPanel(null);
              setSetupOpen(true);
            }}
          />
        )}
        {showSetup && (
          <HarnessOnboarding
            store={store}
            nodeName={
              nodes.nodes.find((node) => node.identity.environmentId === state.node?.environmentId)
                ?.displayName ?? "This computer"
            }
            onFinish={finishSetup}
          />
        )}
        {panel && panel !== "settings" && <WorkspaceDetailPanel view={view} />}
      </WorkspaceShell>
    </ContributionHost>
  );
}
