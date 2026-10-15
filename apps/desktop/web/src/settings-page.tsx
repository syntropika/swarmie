import {
  Settings,
  Bell,
  Keyboard,
  Download,
  Plug,
  Search,
  X,
  UserRound,
  ShieldCheck,
  Terminal,
} from "lucide-react";
import { useState, useId } from "react";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { InputGroup } from "@/components/ui/input-group";
import { Switch } from "@/components/ui/switch";

import { AccountSecurity } from "./account-security";
import { AccountSettings } from "./account-settings";
import { startAction } from "./action-boundary";
import { ChatDirectorySettings } from "./chat-directory-settings";
import { CodingAgentsPanel } from "./coding-agents-panel";
import { ContributionSlot } from "./contribution-host";
import { McpSettings } from "./mcp-settings";
import { NativeHistory } from "./native-history";
import { NodeSettings } from "./node-settings";
import type { NodeClient, NodesState } from "./nodes";
import { shortcutBinding, defaultShortcuts } from "./shortcuts";
import type { Shortcuts } from "./shortcuts";
import { hasText } from "./value-presence";
import type { WorkspaceStore, Workspace } from "./workspace";

const sections = [
  { id: "account", title: "Account", icon: UserRound },
  { id: "security", title: "Account security", icon: ShieldCheck },
  { id: "general", title: "General", icon: Settings },
  { id: "agents", title: "Agent tools", icon: Terminal },
  { id: "notifications", title: "Notifications", icon: Bell },
  { id: "import", title: "Import", icon: Download },
  { id: "shortcuts", title: "Keyboard shortcuts", icon: Keyboard },
  { id: "mcps", title: "MCP servers", icon: Plug },
  { id: "extensions", title: "Extensions", icon: Plug },
  { id: "connections", title: "Nodes", icon: Plug },
] as const;

export function SettingsPage(props: SettingsProps) {
  return <SettingsContent key={props.initialSection ?? "general"} {...props} />;
}

type SettingsProps = {
  initialSection?: "general" | "account" | "security";
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
  initialSection?: "general" | "account" | "security";
  client: NodeClient;
  nodes: NodesState;
  store: WorkspaceStore;
  state: Workspace;
  shortcuts: Shortcuts;
  setShortcuts: (value: Shortcuts) => void;
  onClose: () => void;
  onSetup: () => void;
}) {
  const [section, setSection] = useState<string>(initialSection);
  const inputId = useId();
  const [query, setQuery] = useState("");
  const perform = async (action: () => Promise<unknown>) => {
    try {
      await action();
    } catch (error) {
      store.setError(error);
    }
  };
  const matches = sections.filter((entry) =>
    entry.title.toLowerCase().includes(query.toLowerCase()),
  );
  return (
    <div className="settings-page">
      <aside className="settings-navigation" aria-label="Settings navigation">
        <div className="settings-navigation-heading">
          <h2>Settings</h2>
          <Button variant="ghost" size="icon" aria-label="Back to chat" onClick={onClose}>
            <X size={15} />
          </Button>
        </div>
        <div className="settings-search">
          <InputGroup
            icon={<Search size={14} />}
            aria-label="Search settings"
            placeholder="Search"
            value={query}
            onChange={(event) => {
              setQuery(event.target.value);
            }}
          />
        </div>
        <p className="settings-group-label">Personal</p>
        <nav>
          {matches.map(({ id, title, icon: Icon }) => (
            <Button
              key={id}
              variant={section === id ? "secondary" : "ghost"}
              className="w-full"
              aria-current={section === id ? "page" : undefined}
              onClick={() => {
                setSection(id);
              }}
            >
              <span className="flex w-full items-center gap-2">
                <Icon size={14} />
                <span>{title}</span>
              </span>
            </Button>
          ))}
        </nav>
        {!matches.length && <p className="muted">No matching settings.</p>}
      </aside>
      <main className="settings-content" aria-label="Settings">
        <div className="settings-content-width">
          <h1>{sections.find((entry) => entry.id === section)?.title}</h1>
          {state.error && (
            <div className="settings-error" role="alert">
              <p>{state.error}</p>
              <Button
                variant="ghost"
                size="icon"
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
                  setSection("account");
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
                  label={label}
                  checked={state.notifications[key]}
                  onChange={(value) => {
                    startAction(
                      perform(async () => {
                        await store.setNotifications({ ...state.notifications, [key]: value });
                      }),
                      store.setError.bind(store),
                    );
                  }}
                />
              ))}
            </div>
          )}
          {section === "shortcuts" && (
            <div className="settings-section">
              {(["palette", "newChat", "files", "tools"] as const).map((key) => (
                <label key={key} htmlFor={`${inputId}-${key}`}>
                  {
                    {
                      palette: "Command palette",
                      newChat: "New conversation",
                      files: "Find file",
                      tools: "Workspace tools",
                    }[key]
                  }
                  <Input
                    id={`${inputId}-${key}`}
                    aria-label={`Shortcut for ${key}`}
                    value={shortcuts[key]}
                    readOnly
                    onKeyDown={(e) => {
                      e.stopPropagation();
                      e.preventDefault();
                      try {
                        const binding = shortcutBinding(e.nativeEvent);
                        if (!hasText(binding)) {
                          return;
                        }
                        const next = { ...shortcuts, [key]: binding };
                        if (Object.values(next).filter((v) => v === binding).length > 1) {
                          throw new Error("Choose a shortcut not assigned to another action.");
                        }
                        localStorage.setItem("swarmie.shortcuts", JSON.stringify(next));
                        setShortcuts(next);
                      } catch (error) {
                        store.setError(error);
                      }
                    }}
                  />
                </label>
              ))}
              <Button
                variant="ghost"
                size="sm"
                onClick={() => {
                  localStorage.removeItem("swarmie.shortcuts");
                  setShortcuts({ ...defaultShortcuts });
                }}
              >
                Reset shortcuts
              </Button>
            </div>
          )}
          {section === "import" && (
            <div className="settings-section">
              <NativeHistory store={store} />
            </div>
          )}
          {section === "connections" && <NodeSettings client={client} nodes={nodes} />}
        </div>
      </main>
    </div>
  );
}
