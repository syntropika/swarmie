import { Button, Dropdown, Switch, Tabs, Tooltip, TooltipContent } from "@swarmie/ui-web";
import { KeyRound, Monitor, MoreHorizontal, RefreshCw } from "lucide-react";
import { useState } from "react";

import { startAction, errorMessage, errorReporter } from "~/src/action-boundary";
import {
  AddNodeDialog,
  LocalAccessDialog,
  NodeDetailsDialog,
  routeAddress,
} from "~/src/node-dialogs";
import type { NodeClient, NodesState } from "~/src/nodes";
import { hasText, nonEmptyText } from "~/src/value-presence";

const views = [
  { id: "computer", label: "Control this computer" },
  { id: "devices", label: "Control other devices" },
  { id: "ssh", label: "SSH" },
] as const;
type View = (typeof views)[number]["id"] | "ssh";

export function NodeSettings({ client, nodes }: { client: NodeClient; nodes: NodesState }) {
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
      <Tabs
        selectedKey={activeView}
        onSelectionChange={(next) => {
          if (next === "computer" || next === "devices" || next === "ssh") {
            setView(next);
          }
        }}
      >
        <Tabs.ListContainer>
          <Tabs.List aria-label="Connections view">
            {tabs.map((tab) => (
              <Tabs.Tab key={tab.id} id={tab.id}>
                {tab.label}
                <Tabs.Indicator />
              </Tabs.Tab>
            ))}
          </Tabs.List>
        </Tabs.ListContainer>
        <Tabs.Panel id={activeView}>
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
                    variant="tertiary"
                    size="md"
                    isIconOnly
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
                    size="md"
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
            <p className="mt-4 text-xs leading-relaxed text-muted">
              The app connects to this computer automatically and retries after a temporary
              connection loss. Remote access from other devices is not configured here yet.
            </p>
          )}
          {remoteView && (
            <p className="mt-4 text-xs leading-relaxed text-muted">
              Add a running runtime by address or through configured SSH access. Existing chats keep
              their original node.
            </p>
          )}
        </Tabs.Panel>
      </Tabs>
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
            <Monitor size={19} className="shrink-0 text-muted" />
            <div className="min-w-0 flex-1">
              <p className="truncate text-sm font-medium" title={node.displayName}>
                {node.displayName}
              </p>
              <p className="mt-1 truncate text-xs text-muted" title={description}>
                {description}
              </p>
              <p className="mt-1 text-xs text-muted">{status}</p>
            </div>
            {!node.local && (
              <Switch
                isSelected={connected}
                isDisabled={busy || !node.registered}
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
                aria-label={`Connect ${node.displayName}`}
              >
                <Switch.Content>
                  <Switch.Control>
                    <Switch.Thumb />
                  </Switch.Control>
                  <span className="sr-only">{`Connect ${node.displayName}`}</span>
                </Switch.Content>
              </Switch>
            )}
            <Dropdown>
              <Button
                variant="tertiary"
                size="md"
                isIconOnly
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
          <p className="text-sm text-muted">
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
