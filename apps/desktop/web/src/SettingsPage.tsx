import { useState } from "react";
import { InputGroup } from "@/components/ui/input-group";
import { Settings, Bell, Keyboard, Download, Plug, Search, X, Cable } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { ChatDirectorySettings } from "./ChatDirectorySettings";
import { NativeHistory } from "./NativeHistory";
import { HarnessIcon } from "./HarnessSelect";
import { shortcutBinding, defaultShortcuts, type Shortcuts } from "./shortcuts";
import type { WorkspaceStore, Workspace } from "./workspace";

const sections = [
  { id: "general", title: "General", icon: Settings },
  { id: "notifications", title: "Notifications", icon: Bell },
  { id: "import", title: "Import", icon: Download },
  { id: "shortcuts", title: "Keyboard shortcuts", icon: Keyboard },
  { id: "connections", title: "Connections", icon: Plug },
] as const;

export function SettingsPage({ store, state, shortcuts, setShortcuts, reconnect, onClose }: {
  store: WorkspaceStore;
  state: Workspace;
  shortcuts: Shortcuts;
  setShortcuts: (value: Shortcuts) => void;
  reconnect: (token: string) => Promise<void>;
  onClose: () => void;
}) {
  const [section, setSection] = useState<string>("general");
  const [query, setQuery] = useState("");
  const [token, setToken] = useState("");
  const [working, setWorking] = useState(false);
  const perform = async (action: () => Promise<unknown>) => {
    setWorking(true);
    try { await action(); } catch (error) { store.setError(error); } finally { setWorking(false); }
  };
  const connectAndOpen = reconnect;
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
                              "mpai.shortcuts",
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
                    localStorage.removeItem("mpai.shortcuts");
                    setShortcuts({ ...defaultShortcuts });
                  }}
                >
                  Reset shortcuts
                </Button>
        </div>}
        {section === "import" && <div className="settings-section"><NativeHistory store={store} /></div>}
        {section === "connections" && <div className="settings-section">
              <div className="connection-status">
                <Cable size={20} />
                <div>
                  <strong>
                    {state.connected ? "Connected" : "Connect to your daemon"}
                  </strong>
                  <p>{window.location.origin}</p>
                </div>
                <span
                  className={`connection-dot ${state.connected ? "connected" : ""}`}
                />
              </div>
              <label>
                Daemon token
                <Input
                  type="password"
                  autoComplete="off"
                  value={token}
                  onChange={(e) => setToken(e.target.value)}
                  placeholder="Optional bearer token"
                />
              </label>
              <Button
                variant="ghost"
                className="primary-button"
                disabled={state.busy}
                onClick={() => void connectAndOpen(token).catch((e) => store.setError(e))}
              >
                {state.busy ? "Connecting…" : "Reconnect"}
              </Button>
              <h3>Available harnesses</h3>
              {state.harnesses.some((item) => item.available) ? (
                state.harnesses.filter((item) => item.available).map((h) => (
                  <div className="harness-row" key={h.harness}>
                    <HarnessIcon harness={h.harness} />
                    <span>{h.harness}</span>
                    <span className="muted">
                      {h.authState ?? "Installed"}
                    </span>
                    {h.available && (
                      <Button
                        variant="ghost"
                        className="small-button"
                        disabled={working}
                        onClick={() =>
                          void perform(() => store.probe(h.harness))
                        }
                      >
                        Check readiness
                      </Button>
                    )}
                    {(h.message || h.error) && (
                      <p className="panel-note">{h.message || h.error}</p>
                    )}
                  </div>
                ))
              ) : (
                <p className="muted">
                  Install a harness and make its executable available to the
                  daemon, then reconnect.
                </p>
              )}
              <p className="panel-note">
                Harnesses use their own credentials and provider configuration.
                The token stays in memory.
              </p>
        </div>}
      </div>
    </main>
  </div>;
}
