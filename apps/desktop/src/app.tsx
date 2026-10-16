import { Button } from "@swarmie/ui-web";
import { Home, GitBranch } from "lucide-react";
// Copyright 2026 Syntropika
// SPDX-License-Identifier: Apache-2.0
import { useEffect, useSyncExternalStore } from "react";

import { AccountMenu } from "~/src/account-menu";
import { startAction } from "~/src/action-boundary";
import { ContributionHost, ContributionSlot } from "~/src/contribution-host";
import { HarnessOnboarding } from "~/src/harness-onboarding";
import { nodeKey } from "~/src/node-scope";
import { NodeQueryBoundary } from "~/src/query-boundary";
import { SettingsPage } from "~/src/settings-page";
import { hasText } from "~/src/value-presence";
import { WorkspaceDialogs } from "~/src/workspace-dialogs";
import { WorkspaceHeader } from "~/src/workspace-header";
import { IconButton } from "~/src/workspace-icon-button";
import { nodeClient } from "~/src/workspace-instance";
import { WorkspaceConversation, WorkspaceDetailPanel } from "~/src/workspace-panels";
import { WorkspaceShell } from "~/src/workspace-shell";
import { WorkspaceSidebar } from "~/src/workspace-sidebar";
import type { NodeWorkspaceProps } from "~/src/workspace-state";
import { useWorkspaceState } from "~/src/workspace-state";

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
    shortcuts,
    setShortcuts,
    archived,
    setArchived,
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
        </nav>
        <WorkspaceSidebar view={view} />
        {sidebar && (
          <Button
            variant="tertiary"
            data-workspace-part="sidebar-backdrop"
            aria-label="Close sidebar"
            onClick={() => {
              setSidebar(false);
            }}
          />
        )}
        <WorkspaceConversation view={view} />
        <WorkspaceDialogs view={view} />
        {panel === "settings" && (
          <SettingsPage
            section={settingsSection}
            onSectionChange={setSettingsSection}
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
