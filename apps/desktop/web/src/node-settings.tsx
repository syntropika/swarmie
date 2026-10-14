import { KeyRound, Monitor, MoreHorizontal, RefreshCw } from "lucide-react";
import { useId, useState } from "react";

import { Button } from "@/components/ui/button";
import { Dropdown } from "@/components/ui/dropdown";
import { Switch } from "@/components/ui/switch";
import { Tooltip, TooltipContent } from "@/components/ui/tooltip";

import { startAction, errorMessage, errorReporter } from "./action-boundary";
import { AddNodeDialog, LocalAccessDialog, NodeDetailsDialog, routeAddress } from "./node-dialogs";
import type { NodeClient, NodesState } from "./nodes";
import { hasText, nonEmptyText } from "./value-presence";

const views = [
  { id: "computer", label: "Control this computer" },
  { id: "devices", label: "Control other devices" },
  { id: "ssh", label: "SSH" },
] as const;
type View = (typeof views)[number]["id"] | "ssh";

export function NodeSettings({ client, nodes }: { client: NodeClient; nodes: NodesState }) {
  const id = useId();
  const [view, setView] = useState<View>("computer");
  const [dialog, setDialog] = useState<
    { kind: "add" | "access" } | { kind: "details"; id: string }
  >();
  const [busy, setBusy] = useState(false);
  const [issue, setIssue] = useState("");
  const run = async (action: () => Promise<unknown>) => {
    setBusy(true);
    setIssue("");
    try {
      await action();
    } catch (error) {
      setIssue(errorMessage(error));
    } finally {
      setBusy(false);
    }
  };
  const local = client.localSnapshot();
  const detail = selectedNode(nodes, dialog);
  const tabs = views;
  const activeView = tabs.some((tab) => tab.id === view) ? view : "devices";
  const visible = visibleNodes(nodes, activeView);
  const remoteView = activeView !== "computer";
  return (
    <div>
      <div
        role="tablist"
        aria-label="Connections view"
        className="bg-control-secondary mb-7 flex w-fit max-w-full gap-1 overflow-x-auto rounded-lg p-1"
      >
        {tabs.map((tab, index) => (
          <Button
            key={tab.id}
            id={`${id}-${tab.id}`}
            role="tab"
            aria-selected={activeView === tab.id}
            aria-controls={`${id}-panel`}
            tabIndex={activeView === tab.id ? 0 : -1}
            variant={activeView === tab.id ? "secondary" : "ghost"}
            size="sm"
            className="shrink-0"
            onClick={() => {
              setView(tab.id);
            }}
            onKeyDown={(event) => {
              const next = (() => {
                if (event.key === "ArrowRight") {
                  return (index + 1) % tabs.length;
                }
                if (event.key === "ArrowLeft") {
                  return (index + tabs.length - 1) % tabs.length;
                }
                if (event.key === "Home") {
                  return 0;
                }
                if (event.key === "End") {
                  return tabs.length - 1;
                }
                return undefined;
              })();
              if (next !== undefined) {
                event.preventDefault();
                setView(tabs[next].id);
                document.querySelector<HTMLElement>(`[id="${id}-${tabs[next].id}"]`)?.focus();
              }
            }}
          >
            {tab.label}
          </Button>
        ))}
      </div>
      <section id={`${id}-panel`} role="tabpanel" aria-labelledby={`${id}-${activeView}`}>
        <div className="mb-4 flex items-center justify-between gap-3">
          <h2 className="text-sm font-medium">
            {(() => {
              if (activeView === "computer") {
                return "This computer";
              }
              if (activeView === "ssh") {
                return "SSH connections from this computer";
              }
              return "Devices you can control from this computer";
            })()}
          </h2>
          {remoteView && (
            <div className="flex shrink-0 items-center gap-1">
              <Tooltip>
                <Button
                  variant="ghost"
                  size="icon-sm"
                  aria-label="Refresh nodes"
                  disabled={busy || nodes.loading || !local.connected}
                  onClick={() => {
                    startAction(
                      run(async () => {
                        await client.refresh();
                      }),
                      errorReporter(setIssue),
                    );
                  }}
                >
                  <RefreshCw
                    size={15}
                    className={nodes.loading ? "animate-spin motion-reduce:animate-none" : ""}
                  />
                </Button>
                <TooltipContent>Refresh</TooltipContent>
              </Tooltip>
              {!!visible.length && (
                <Button
                  variant="secondary"
                  size="sm"
                  disabled={busy || !local.connected}
                  onClick={() => {
                    setDialog({ kind: "add" });
                  }}
                >
                  Add
                </Button>
              )}
            </div>
          )}
        </div>
        {(hasText(issue) || hasText(nodes.error)) && (
          <p className="mb-4 text-sm break-words text-danger" role="alert">
            {nonEmptyText(issue) ?? nodes.error}
          </p>
        )}
        <NodeList
          client={client}
          visible={visible}
          remoteView={remoteView}
          busy={busy}
          run={run}
          setDialog={setDialog}
          local={local}
          reportError={errorReporter(setIssue)}
        />

        {activeView === "computer" && (
          <p className="text-muted mt-4 text-xs leading-relaxed">
            The app connects to this computer automatically and retries after a temporary connection
            loss. Remote access from other devices is not configured here yet.
          </p>
        )}
        {remoteView && (
          <p className="text-muted mt-4 text-xs leading-relaxed">
            Add a running runtime by address or through configured SSH access. Existing chats keep
            their original node.
          </p>
        )}
      </section>
      {dialog?.kind === "add" && (
        <AddNodeDialog
          client={client}
          initialMethod={activeView === "ssh" ? "ssh" : "direct"}
          onClose={() => {
            setDialog(undefined);
          }}
        />
      )}
      {dialog?.kind === "access" && (
        <LocalAccessDialog
          client={client}
          onClose={() => {
            setDialog(undefined);
          }}
        />
      )}
      {detail && (
        <NodeDetailsDialog
          node={detail}
          client={client}
          onClose={() => {
            setDialog(undefined);
          }}
          onAccess={() => {
            setDialog({ kind: "access" });
          }}
        />
      )}
    </div>
  );
}

type NodeDialog = { kind: "add" | "access" } | { kind: "details"; id: string };
function NodeList({
  client,
  visible,
  remoteView,
  busy,
  run,
  setDialog,
  local,
  reportError,
}: {
  client: NodeClient;
  visible: NodesState["nodes"];
  remoteView: boolean;
  busy: boolean;
  run: (action: () => Promise<unknown>) => Promise<void>;
  setDialog: (dialog: NodeDialog) => void;
  local: ReturnType<NodeClient["localSnapshot"]>;
  reportError: (error: unknown) => void;
}) {
  return (
    <ul aria-label={remoteView ? "Remote nodes" : "Local node"} className="divide-y divide-border">
      {visible.map((node) => {
        const status = client.status(node);
        const connected = status === "Connected";
        const description = (() => {
          if (node.local) {
            return "Connects automatically";
          }
          if (node.connections.length === 1) {
            return routeAddress(node.connections[0]);
          }
          return `${node.connections.length} saved connections`;
        })();
        return (
          <li key={node.identity.environmentId} className="flex items-center gap-3 py-4">
            <Monitor size={19} className="text-muted shrink-0" />
            <div className="min-w-0 flex-1">
              <p className="truncate text-sm font-medium" title={node.displayName}>
                {node.displayName}
              </p>
              <p className="text-muted mt-1 truncate text-xs" title={description}>
                {description}
              </p>
              <p className="text-muted mt-1 text-xs">{status}</p>
            </div>
            {!node.local && (
              <Switch
                label={`Connect ${node.displayName}`}
                checked={connected}
                disabled={busy || !node.registered}
                onChange={(value) => {
                  if (value) {
                    startAction(
                      run(async () => {
                        await client.connect(node.identity.environmentId);
                      }),
                      reportError,
                    );
                  } else {
                    client.disconnect(node.identity.environmentId);
                  }
                }}
                hideLabel
              />
            )}
            <Dropdown>
              <Button
                variant="ghost"
                size="icon-sm"
                aria-label={`${node.displayName} actions`}
                disabled={busy}
              >
                <MoreHorizontal size={17} />
              </Button>
              <Dropdown.Popover placement="bottom end">
                <Dropdown.Menu aria-label={`${node.displayName} actions`}>
                  <Dropdown.Item
                    id="details"
                    onAction={() => {
                      setDialog({ kind: "details", id: node.identity.environmentId });
                    }}
                  >
                    Details
                  </Dropdown.Item>
                  <Dropdown.Item
                    id="open"
                    isDisabled={!node.registered}
                    onAction={() => {
                      startAction(
                        run(async () => {
                          await client.select(node.identity.environmentId);
                        }),
                        reportError,
                      );
                    }}
                  >
                    Open node
                  </Dropdown.Item>
                </Dropdown.Menu>
              </Dropdown.Popover>
            </Dropdown>
          </li>
        );
      })}
      {!visible.length && (
        <li className="flex flex-col items-center gap-3 py-8 text-center">
          <Monitor size={24} className="text-muted" />
          <p className="text-muted text-sm">
            {remoteView
              ? "No devices added yet"
              : (nonEmptyText(local.error) ?? "Connecting automatically…")}
          </p>
          {remoteView ? (
            <Button
              variant="secondary"
              disabled={busy || !local.connected}
              onClick={() => {
                setDialog({ kind: "add" });
              }}
            >
              Add
            </Button>
          ) : (
            /token/iu.test(local.error) && (
              <Button
                variant="secondary"
                onClick={() => {
                  setDialog({ kind: "access" });
                }}
              >
                <KeyRound size={14} />
                Connection access
              </Button>
            )
          )}
        </li>
      )}
    </ul>
  );
}

function visibleNodes(nodes: NodesState, view: string) {
  return nodes.nodes.filter((node) =>
    view === "computer"
      ? node.local
      : !node.local &&
        (view !== "ssh" || node.connections.some((route) => route.transport.kind === "ssh")),
  );
}

function selectedNode(nodes: NodesState, dialog?: NodeDialog) {
  return dialog?.kind === "details"
    ? nodes.nodes.find((node) => node.identity.environmentId === dialog.id)
    : undefined;
}
