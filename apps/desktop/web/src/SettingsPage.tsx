import { AccountSettings } from "./AccountSettings";
import { useState, useEffect } from "react";
import { NodeSettings } from "./NodeSettings";
import type { NodeClient, NodesState } from "./nodes";
import { InputGroup } from "@/components/ui/input-group";
import { Settings, Bell, Keyboard, Download, Plug, Search, X, UserRound } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { ChatDirectorySettings } from "./ChatDirectorySettings";
import { NativeHistory } from "./NativeHistory";
import { shortcutBinding, defaultShortcuts, type Shortcuts } from "./shortcuts";
import type { WorkspaceStore, Workspace } from "./workspace";

const sections = [
  { id: "account", title: "Account", icon: UserRound },
  { id: "general", title: "General", icon: Settings },
  { id: "notifications", title: "Notifications", icon: Bell },
  { id: "import", title: "Import", icon: Download },
  { id: "shortcuts", title: "Keyboard shortcuts", icon: Keyboard },
  { id: "connections", title: "Nodes", icon: Plug },
] as const;

export function SettingsPage({ store, state, shortcuts, setShortcuts, onClose, client, nodes, initialSection = "general" }: {
  initialSection?: "general" | "account";
  client: NodeClient;
  nodes: NodesState;
  store: WorkspaceStore;
  state: Workspace;
  shortcuts: Shortcuts;
  setShortcuts: (value: Shortcuts) => void;
  onClose: () => void;
}) {
  const [section, setSection] = useState<string>(initialSection);
  useEffect(() => setSection(initialSection), [initialSection]);
  const [query, setQuery] = useState("");
  const [working, setWorking] = useState(false);
  const perform = async (action: () => Promise<unknown>) => {
    setWorking(true);
    try { await action(); } catch (error) { store.setError(error); } finally { setWorking(false); }
  };
  const matches = sections.filter((entry) => entry.title.toLowerCase().includes(query.toLowerCase()));
  return <div className="settings-page">
    <aside className="settings-navigation" aria-label="Settings navigation">
      <div className="settings-navigation-heading"><h2>Settings</h2><Button variant="ghost" size="icon" aria-label="Back to chat" onClick={onClose}><X size={15} /></Button></div>
      <InputGroup className="settings-search" icon={<Search size={14} />} aria-label="Search settings" placeholder="Search" value={query} onChange={(event) => setQuery(event.target.value)} />
      <p className="settings-group-label">Personal</p>
      <nav>{matches.map(({ id, title, icon: Icon }) => <Button key={id} variant={section === id ? "secondary" : "ghost"} className={`settings-nav-item ${section === id ? "active" : ""}`} aria-current={section === id ? "page" : undefined} onClick={() => setSection(id)}><Icon size={14} /><span>{title}</span></Button>)}</nav>
      {!matches.length && <p className="muted">No matching settings.</p>}
    </aside>
    <main className="settings-content" aria-label="Settings">
      <div className="settings-content-width">
        <h1>{sections.find((entry) => entry.id === section)?.title}</h1>
        {state.error && <div className="settings-error" role="alert"><p>{state.error}</p><Button variant="ghost" size="icon" aria-label="Dismiss settings error" onClick={() => store.dismissError()}><X size={14} /></Button></div>}
        {section === "account" && <div className="settings-section"><AccountSettings /></div>}
        {section === "general" && <div className="settings-section"><ChatDirectorySettings store={store} directory={state.chatDirectory} custom={!!state.customChatDirectory} connected={state.connected} /></div>}
        {section === "notifications" && <div className="settings-section settings-notifications">
              {(
                [
                  ["enabled", "Enable desktop notifications"],
                  ["completion", "When an agent finishes"],
                  ["input", "When an agent needs an answer"],
                  ["failure", "When an agent encounters an error"],
                  [
                    "backgroundOnly",
                    "Only while this app is in the background",
                  ],
                ] as const
              ).map(([key, label]) => (
                <Switch key={key} label={label} checked={state.notifications[key]}
                  onChange={(value) => void perform(() => store.setNotifications({ ...state.notifications, [key]: value }))} />
              ))}
        </div>}
        {section === "shortcuts" && <div className="settings-section">
                {(Object.keys(shortcuts) as (keyof typeof shortcuts)[]).map(
                  (key) => (
                    <label key={key}>
                      {
                        {
                          palette: "Command palette",
                          newChat: "New conversation",
                          files: "Find file",
                          tools: "Workspace tools",
                        }[key]
                      }
                      <Input
                        aria-label={`Shortcut for ${key}`}
                        value={shortcuts[key]}
                        readOnly
                        onKeyDown={(e) => {
                          e.stopPropagation();
                          e.preventDefault();
                          try {
                            const binding = shortcutBinding(e.nativeEvent);
                            if (!binding) return;
                            const next = { ...shortcuts, [key]: binding };
                            if (
                              Object.values(next).filter((v) => v === binding)
                                .length > 1
                            )
                              throw new Error(
                                "Choose a shortcut not assigned to another action.",
                              );
                            localStorage.setItem(
                              "swarmie.shortcuts",
                              JSON.stringify(next),
                            );
                            setShortcuts(next);
                          } catch (error) {
                            store.setError(error);
                          }
                        }}
                      />
                    </label>
                  ),
                )}
                <Button
                  variant="ghost"
                  className="small-button"
                  onClick={() => {
                    localStorage.removeItem("swarmie.shortcuts");
                    setShortcuts({ ...defaultShortcuts });
                  }}
                >
                  Reset shortcuts
                </Button>
        </div>}
        {section === "import" && <div className="settings-section"><NativeHistory store={store} /></div>}
        {section === "connections" && <NodeSettings client={client} nodes={nodes} />}
      </div>
    </main>
  </div>;
}
