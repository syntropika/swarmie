import { Button, SearchField, Switch } from "@swarmie/ui-web";
import { X } from "lucide-react";
import { useState } from "react";

import { AccountSecurity } from "~/src/account-security";
import { AccountSettings } from "~/src/account-settings";
import { useAccount } from "~/src/account-state";
import { startAction } from "~/src/action-boundary";
import { ChatDirectorySettings } from "~/src/chat-directory-settings";
import { CodingAgentsPanel } from "~/src/coding-agents-panel";
import { ContributionSlot } from "~/src/contribution-host";
import { McpSettings } from "~/src/mcp-settings";
import { NodeSettings } from "~/src/node-settings";
import type { NodeClient, NodesState } from "~/src/nodes";
import { settingsSections, visibleSettingsSections } from "~/src/settings-sections";
import type { SettingsSection } from "~/src/settings-sections";
import { ShortcutSettings } from "~/src/shortcut-settings";
import type { Shortcuts } from "~/src/shortcuts";
import { hasText } from "~/src/value-presence";
import type { WorkspaceStore, Workspace } from "~/src/workspace";

export function SettingsPage(props: SettingsProps) {
  return <SettingsContent key={props.initialSection ?? "general"} {...props} />;
}

type SettingsProps = {
  initialSection?: SettingsSection;
  client: NodeClient;
  nodes: NodesState;
  store: WorkspaceStore;
  state: Workspace;
  shortcuts: Shortcuts;
  setShortcuts: (value: Shortcuts) => void;
  onClose: () => void;
  onSetup: () => void;
};

function SettingsContent({
  store,
  state,
  shortcuts,
  setShortcuts,
  onClose,
  onSetup,
  client,
  nodes,
  initialSection = "general",
}: {
  initialSection?: SettingsSection;
  client: NodeClient;
  nodes: NodesState;
  store: WorkspaceStore;
  state: Workspace;
  shortcuts: Shortcuts;
  setShortcuts: (value: Shortcuts) => void;
  onClose: () => void;
  onSetup: () => void;
}) {
  const { account } = useAccount();
  const signedIn = account?.status === "signedIn";
  const [selectedSection, setSelectedSection] = useState<string>(initialSection);
  const section = selectedSection === "security" && !signedIn ? "account" : selectedSection;
  const [query, setQuery] = useState("");
  const perform = async (action: () => Promise<unknown>) => {
    try {
      await action();
    } catch (error) {
      store.setError(error);
    }
  };
  const matches = visibleSettingsSections(signedIn).filter((entry) =>
    entry.title.toLowerCase().includes(query.toLowerCase()),
  );
  return (
    <div className="settings-page">
      <aside className="settings-navigation" aria-label="Settings navigation">
        <div className="settings-navigation-heading">
          <h2>Settings</h2>
          <Button
            variant="tertiary"
            size="md"
            isIconOnly
            aria-label="Back to chat"
            onClick={onClose}
          >
            <X size={15} />
          </Button>
        </div>
        <div className="settings-search">
          <SearchField fullWidth aria-label="Search settings" value={query} onChange={setQuery}>
            <SearchField.Group>
              <SearchField.SearchIcon />
              <SearchField.Input className="min-w-0" placeholder="Search" />
              <SearchField.ClearButton aria-label="Clear settings search" />
            </SearchField.Group>
          </SearchField>
        </div>
        <div className="settings-navigation-groups">
          {(["Personal", "Integrations", "Coding"] as const).map((group) => {
            const entries = matches.filter((entry) => entry.group === group);
            if (!entries.length) {
              return null;
            }
            return (
              <div key={group} className="settings-navigation-group">
                <p className="settings-group-label">{group}</p>
                <nav aria-label={`${group} settings`}>
                  {entries.map(({ id, title, icon: Icon }) => (
                    <Button
                      key={id}
                      variant={section === id ? "secondary" : "tertiary"}
                      className="w-full"
                      aria-current={section === id ? "page" : undefined}
                      onClick={() => {
                        setSelectedSection(id);
                      }}
                    >
                      <span className="flex w-full items-center gap-2">
                        <Icon size={14} />
                        <span>{title}</span>
                      </span>
                    </Button>
                  ))}
                </nav>
              </div>
            );
          })}
        </div>
        {!matches.length && <p className="muted">No matching settings.</p>}
      </aside>
      <main className="settings-content" aria-label="Settings">
        <div className="settings-content-width">
          <h1>{settingsSections.find((entry) => entry.id === section)?.title}</h1>
          {state.error && (
            <div className="settings-error" role="alert">
              <p>{state.error}</p>
              <Button
                variant="tertiary"
                size="md"
                isIconOnly
                aria-label="Dismiss settings error"
                onClick={() => {
                  store.dismissError();
                }}
              >
                <X size={14} />
              </Button>
            </div>
          )}
          {section === "mcps" && (
            <div className="settings-section">
              <McpSettings store={store} state={state} />
            </div>
          )}
          {section === "account" && (
            <div className="settings-section">
              <AccountSettings />
            </div>
          )}
          {section === "security" && (
            <div className="settings-section">
              <AccountSecurity
                onProfile={() => {
                  setSelectedSection("account");
                }}
              />
            </div>
          )}
          {section === "extensions" && <ContributionSlot name="settings.sections" />}
          {section === "general" && (
            <div className="settings-section">
              <ChatDirectorySettings
                store={store}
                directory={state.chatDirectory}
                custom={hasText(state.customChatDirectory)}
                connected={state.connected}
              />
            </div>
          )}
          {section === "agents" && (
            <div className="settings-section">
              <p className="onboarding-description">
                Choose which AI tools are available for new chats on this computer.
              </p>
              <CodingAgentsPanel
                store={store}
                nodeName={
                  nodes.nodes.find(
                    (node) => node.identity.environmentId === state.node?.environmentId,
                  )?.displayName ?? "This computer"
                }
              />
              <Button variant="outline" onClick={onSetup}>
                Open setup
              </Button>
            </div>
          )}
          {section === "notifications" && (
            <div className="settings-section settings-notifications">
              {(
                [
                  ["enabled", "Enable desktop notifications"],
                  ["completion", "When an agent finishes"],
                  ["input", "When an agent needs an answer"],
                  ["failure", "When an agent encounters an error"],
                  ["backgroundOnly", "Only while this app is in the background"],
                ] as const
              ).map(([key, label]) => (
                <Switch
                  key={key}
                  isSelected={state.notifications[key]}
                  onChange={(value) => {
                    startAction(
                      perform(async () => {
                        await store.setNotifications({ ...state.notifications, [key]: value });
                      }),
                      store.setError.bind(store),
                    );
                  }}
                  aria-label={label}
                >
                  <Switch.Content>
                    <Switch.Control>
                      <Switch.Thumb />
                    </Switch.Control>
                    {label}
                  </Switch.Content>
                </Switch>
              ))}
            </div>
          )}
          {section === "shortcuts" && (
            <ShortcutSettings shortcuts={shortcuts} onChange={setShortcuts} />
          )}
          {section === "connections" && <NodeSettings client={client} nodes={nodes} />}
        </div>
      </main>
    </div>
  );
}
