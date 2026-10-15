import { KeyRound, Monitor, Trash2, X } from "lucide-react";
import { useState, useId } from "react";
import type { ReactNode } from "react";

import { startAction, errorReporter } from "~/src/action-boundary";
import { Button } from "~/src/components/ui/button";
import { Dialog } from "~/src/components/ui/dialog";
import { Input } from "~/src/components/ui/input";
import { HarnessIcon } from "~/src/harness-select";
import { NodeStartupControls } from "~/src/node-startup-controls";
import type { ClientNode, NodeClient, NodeRoute, SshReadiness } from "~/src/nodes";
import { hasText, nonEmptyText } from "~/src/value-presence";

export function routeAddress(route: NodeRoute): string {
  if (route.transport.kind === "ssh") {
    return `${route.transport.host ?? "SSH connection"} · ${nonEmptyText(route.transport.socketPath) ?? "Default runtime"}`;
  }
  return route.transport.path ?? route.transport.url ?? "Saved connection";
}
function Frame({
  title,
  busy,
  onClose,
  children,
}: {
  title: string;
  busy: boolean;
  onClose: () => void;
  children: ReactNode;
}) {
  return (
    <Dialog label={title} onClose={onClose} busy={busy} className="max-w-[480px]">
      <header className="mb-5 flex items-center justify-between gap-3">
        <h2 className="min-w-0 truncate text-lg font-semibold" title={title}>
          {title}
        </h2>
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label="Close node dialog"
          disabled={busy}
          onClick={onClose}
        >
          <X size={16} />
        </Button>
      </header>
      {children}
    </Dialog>
  );
}
const errorText = (error: unknown) => (error instanceof Error ? error.message : String(error));

function sshReadinessText(readiness: SshReadiness): string {
  switch (readiness.status) {
    case "ready": {
      return `Runtime ready${hasText(readiness.version) ? ` · ${readiness.version}` : ""}`;
    }
    case "not-installed": {
      return "Install Swarmie runtime on the remote computer, then check again.";
    }
    case "ssh-client-unavailable": {
      return "Install OpenSSH on this computer, then check again.";
    }
    case "ssh-unavailable": {
      return "Check the SSH host, network, host trust and configured authentication, then check again.";
    }
    case "invalid-installation": {
      return "Check the remote Swarmie installation and shell startup output, then check again.";
    }
    case "incompatible-runtime": {
      return "The remote runtime could not be verified. Update its Swarmie installation, then check again.";
    }
    case "runtime-unavailable": {
      return "Check that the runtime service is running on the remote computer and the socket path is correct, then check again.";
    }
    default: {
      return "Check this connection again.";
    }
  }
}

export function AddNodeDialog({
  client,
  onClose,
  initialMethod = "direct",
}: {
  client: NodeClient;
  onClose: () => void;
  initialMethod?: "direct" | "ssh";
}) {
  const fieldId = useId();
  const [method, setMethod] = useState<"direct" | "ssh">(initialMethod);
  const [name, setName] = useState("");
  const [address, setAddress] = useState("");
  const [host, setHost] = useState("");
  const [socket, setSocket] = useState("");
  const [token, setToken] = useState("");
  const [readiness, setReadiness] = useState<SshReadiness>();
  const [busy, setBusy] = useState(false);
  const [preparing, setPreparing] = useState(false);
  const [issue, setIssue] = useState("");
  const ssh = method === "ssh";
  const check = async () => {
    setBusy(true);
    setIssue("");
    setReadiness(undefined);
    try {
      setReadiness(await client.inspectSsh(host.trim(), socket.trim()));
    } catch (error) {
      setIssue(errorText(error));
    } finally {
      setBusy(false);
    }
  };
  const prepare = async (action: "install" | "start") => {
    if (busy || !name.trim()) {
      return;
    }
    setBusy(true);
    setPreparing(true);
    setIssue("");
    setReadiness(undefined);
    try {
      const verified = await client.prepareSsh(name.trim(), host.trim(), socket.trim(), action);
      if (!verified.ready || !verified.runtimeIdentity) {
        throw new Error("The remote runtime could not be verified. Check the connection again.");
      }
      setReadiness(verified);
      onClose();
    } catch (error) {
      setReadiness(undefined);
      setIssue(
        `${errorText(error)} Check the connection before trying again; remote preparation may have completed.`,
      );
    } finally {
      setBusy(false);
      setPreparing(false);
    }
  };
  const pairNode = async () => {
    const identity = readiness?.runtimeIdentity;
    if (busy || (ssh && (readiness?.ready !== true || identity === undefined))) {
      return;
    }
    setBusy(true);
    setIssue("");
    try {
      if (ssh) {
        if (identity === undefined) {
          return;
        }
        await client.pairSsh(name.trim(), host.trim(), socket.trim(), identity);
      } else {
        await client.pair(name.trim(), address.trim(), token);
      }
      onClose();
    } catch (error) {
      setIssue(errorText(error));
      if (ssh) {
        setReadiness(undefined);
      }
    } finally {
      setBusy(false);
    }
  };
  return (
    <Frame title="Add node" busy={busy} onClose={onClose}>
      <p className="text-muted mb-5 text-sm leading-relaxed">
        Connect a computer or prepare its runtime over SSH.
      </p>
      <form
        className="grid gap-4"
        aria-busy={busy}
        onSubmit={(event) => {
          event.preventDefault();
          startAction(pairNode(), errorReporter(setIssue));
        }}
      >
        <label className="grid gap-1.5 text-xs font-medium" htmlFor={`${fieldId}-1`}>
          Connection name
          <Input
            id={`${fieldId}-1`}
            autoFocus
            placeholder="Work computer"
            required
            disabled={busy}
            value={name}
            onChange={(event) => {
              setName(event.target.value);
            }}
          />
        </label>
        <label className="grid gap-1.5 text-xs font-medium" htmlFor={`${fieldId}-2`}>
          Connection method
          <select
            id={`${fieldId}-2`}
            className="bg-background h-9 rounded-md border border-border px-3 text-sm"
            disabled={busy}
            value={method}
            onChange={(event) => {
              const next = event.target.value;
              if (next !== "direct" && next !== "ssh") {
                return;
              }
              setMethod(next);
              setReadiness(undefined);
              setIssue("");
              setToken("");
            }}
          >
            <option value="direct">Address</option>
            <option value="ssh">SSH</option>
          </select>
        </label>
        {ssh ? (
          <>
            <label className="grid gap-1.5 text-xs font-medium" htmlFor={`${fieldId}-3`}>
              SSH host
              <Input
                id={`${fieldId}-3`}
                placeholder="user@server or SSH alias"
                required
                disabled={busy}
                value={host}
                onChange={(event) => {
                  setHost(event.target.value);
                  setReadiness(undefined);
                }}
              />
            </label>
            <details className="text-xs">
              <summary className="text-muted cursor-pointer">Advanced</summary>
              <label className="mt-3 grid gap-1.5 text-xs font-medium" htmlFor={`${fieldId}-4`}>
                Runtime socket path{" "}
                <span className="text-muted font-normal">Optional override</span>
                <Input
                  id={`${fieldId}-4`}
                  aria-label="Runtime socket path"
                  placeholder="Use the default runtime"
                  disabled={busy}
                  value={socket}
                  onChange={(event) => {
                    setSocket(event.target.value);
                    setReadiness(undefined);
                  }}
                />
              </label>
            </details>
            <p className="text-muted text-xs leading-relaxed">
              Use the SSH access configured on this computer. No account is required. Checking
              leaves the remote runtime unchanged.
            </p>
            <Button
              type="button"
              variant="outline"
              disabled={busy || !host.trim()}
              onClick={() => {
                startAction(check(), errorReporter(setIssue));
              }}
            >
              {(() => {
                if (busy) {
                  if (preparing) {
                    return "Preparing runtime…";
                  }
                  return "Checking…";
                }
                return "Check connection";
              })()}
            </Button>
            {readiness && (
              <output className="text-muted text-sm">{sshReadinessText(readiness)}</output>
            )}
            {readiness?.preparation && (
              <div className="grid gap-3 rounded-md border border-border p-3">
                {readiness.preparation.available ? (
                  <>
                    <p className="text-muted text-xs leading-relaxed">
                      {readiness.preparation.action === "install"
                        ? `Install the runtime for your remote user${hasText(readiness.preparation.installPath) ? ` in ${readiness.preparation.installPath}` : ""}, start it and connect. No administrator access is required.`
                        : "Start or attach to the default runtime and connect. Active work will not be restarted."}{" "}
                      The runtime continues after this app closes. Startup when the computer boots
                      is configured separately.
                    </p>
                    <Button
                      type="button"
                      disabled={busy || !name.trim()}
                      onClick={() => {
                        const action = readiness.preparation?.action;
                        if (action === undefined) {
                          return;
                        }
                        startAction(prepare(action), errorReporter(setIssue));
                      }}
                    >
                      {readiness.preparation.action === "install"
                        ? "Install and connect"
                        : "Start and connect"}
                    </Button>
                  </>
                ) : (
                  <p className="text-muted text-xs">{readiness.preparation.reason}</p>
                )}
              </div>
            )}
            {preparing && (
              <output className="text-muted text-sm">
                Preparing and verifying the remote runtime…
              </output>
            )}
          </>
        ) : (
          <>
            <label className="grid gap-1.5 text-xs font-medium" htmlFor={`${fieldId}-5`}>
              Address
              <Input
                id={`${fieldId}-5`}
                placeholder="http://localhost:7490"
                required
                disabled={busy}
                value={address}
                onChange={(event) => {
                  setAddress(event.target.value);
                }}
              />
            </label>
            <label className="grid gap-1.5 text-xs font-medium" htmlFor={`${fieldId}-6`}>
              Access token <span className="text-muted font-normal">Optional</span>
              <Input
                id={`${fieldId}-6`}
                type="password"
                aria-label="Access token"
                autoComplete="off"
                disabled={busy}
                value={token}
                onChange={(event) => {
                  setToken(event.target.value);
                }}
              />
            </label>
          </>
        )}
        {hasText(issue) && (
          <p className="text-sm text-danger" role="alert">
            {issue}
          </p>
        )}
        <p className="text-muted text-xs leading-relaxed">
          The connection is verified before it is saved. Credentials stay on this computer.
        </p>
        <footer className="mt-2 flex justify-end gap-2">
          <Button variant="ghost" disabled={busy} onClick={onClose}>
            Cancel
          </Button>
          <Button
            type="submit"
            disabled={
              busy ||
              !name.trim() ||
              (ssh ? !(readiness?.ready === true) || !readiness.runtimeIdentity : !address.trim())
            }
          >
            {(() => {
              if (busy) {
                if (preparing) {
                  return "Preparing…";
                }
                return "Connecting…";
              }
              return "Add node";
            })()}
          </Button>
        </footer>
      </form>
    </Frame>
  );
}

export function LocalAccessDialog({
  client,
  onClose,
}: {
  client: NodeClient;
  onClose: () => void;
}) {
  const fieldId = useId();
  const [token, setToken] = useState("");
  const [busy, setBusy] = useState(false);
  const [issue, setIssue] = useState("");
  const saveToken = async () => {
    if (busy) {
      return;
    }
    setBusy(true);
    setIssue("");
    try {
      await client.reconnect(token);
      const local = client.localSnapshot();
      if (!local.connected) {
        throw new Error(nonEmptyText(local.error) ?? "The local connection is not available yet.");
      }
      onClose();
    } catch (error) {
      setIssue(errorText(error));
    } finally {
      setBusy(false);
    }
  };
  return (
    <Frame title="Local connection access" busy={busy} onClose={onClose}>
      <p className="text-muted mb-5 text-sm leading-relaxed">
        This computer connects automatically. If its runtime requires a token, enter it here. The
        token stays in memory.
      </p>
      <form
        className="grid gap-4"
        onSubmit={(event) => {
          event.preventDefault();
          startAction(saveToken(), errorReporter(setIssue));
        }}
      >
        <label className="grid gap-1.5 text-xs font-medium" htmlFor={`${fieldId}-1`}>
          Local access token
          <Input
            id={`${fieldId}-1`}
            autoFocus
            type="password"
            autoComplete="off"
            value={token}
            disabled={busy}
            onChange={(event) => {
              setToken(event.target.value);
            }}
          />
        </label>
        {hasText(issue) && (
          <p className="text-sm text-danger" role="alert">
            {issue}
          </p>
        )}
        <footer className="mt-2 flex justify-end gap-2">
          <Button variant="ghost" disabled={busy} onClick={onClose}>
            Cancel
          </Button>
          <Button type="submit" disabled={busy}>
            {busy ? "Verifying…" : "Save token"}
          </Button>
        </footer>
      </form>
    </Frame>
  );
}

export function NodeDetailsDialog({
  node,
  client,
  onClose,
  onAccess,
}: {
  node: ClientNode;
  client: NodeClient;
  onClose: () => void;
  onAccess: () => void;
}) {
  const [busy, setBusy] = useState(false);
  const [issue, setIssue] = useState("");
  const state = client.nodeSnapshot(node);
  const status = client.status(node);
  const run = async (action: () => Promise<unknown>) => {
    setBusy(true);
    setIssue("");
    try {
      await action();
    } catch (error) {
      setIssue(errorText(error));
    } finally {
      setBusy(false);
    }
  };
  const lastSeen =
    node.lastVerifiedAtMs > 0
      ? new Date(node.lastVerifiedAtMs).toLocaleString()
      : "Not yet verified";
  return (
    <Frame title={node.displayName} busy={busy} onClose={onClose}>
      <div className="mb-5 flex items-center gap-3">
        <Monitor size={20} className="text-muted" />
        <div className="min-w-0">
          <p className="text-sm">{status}</p>
          <p className="text-muted text-xs">
            {node.local ? "Connects automatically on this computer" : "Remote node"}
          </p>
        </div>
      </div>
      {(hasText(issue) || hasText(state.error)) && (
        <p className="mb-4 text-sm break-words text-danger" role="alert">
          {nonEmptyText(issue) ?? state.error}
        </p>
      )}
      <dl className="mb-5 grid grid-cols-[auto_minmax(0,1fr)] gap-x-5 gap-y-2 text-xs">
        <dt className="text-muted">Last verified</dt>
        <dd className="text-right">{lastSeen}</dd>
        <dt className="text-muted">Connection</dt>
        <dd className="text-right">
          {node.local
            ? "Automatic"
            : `${node.connections.length} saved route${node.connections.length === 1 ? "" : "s"}`}
        </dd>
      </dl>
      {node.connections.length > 0 && (
        <section aria-label="Saved connections" className="mb-5 border-t border-border pt-4">
          <h3 className="mb-2 text-sm font-medium">Connections</h3>
          <div className="divide-y divide-border">
            {node.connections.map((route) => (
              <div key={route.name} className="flex items-center gap-3 py-3">
                <div className="min-w-0 flex-1">
                  <p className="truncate text-sm">{route.name}</p>
                  <p className="text-muted text-xs break-all">{routeAddress(route)}</p>
                </div>
                <Button
                  variant="ghost"
                  size="icon-sm"
                  aria-label={`Remove connection ${route.name}`}
                  disabled={busy}
                  onClick={() => {
                    startAction(
                      run(async () => {
                        await client.removeRoute(route.name, route.revision);
                      }),
                      errorReporter(setIssue),
                    );
                  }}
                >
                  <Trash2 size={15} />
                </Button>
              </div>
            ))}
          </div>
        </section>
      )}
      <NodeStartupControls
        key={node.identity.environmentId}
        node={node}
        client={client}
        busy={busy}
        run={run}
      />
      <section aria-label="Node agent tools" className="border-t border-border pt-4">
        <h3 className="mb-2 text-sm font-medium">Installed agent tools</h3>
        {state.harnesses
          .filter((harness) => harness.available)
          .map((harness) => (
            <div key={harness.harness} className="flex flex-wrap items-center gap-2 py-2">
              <HarnessIcon harness={harness.harness} />
              <span className="text-sm">{harness.harness}</span>
              <span className="text-muted text-xs">{harness.authState ?? "Installed"}</span>
              <Button
                className="ml-auto"
                variant="ghost"
                size="sm"
                disabled={busy || !state.connected}
                onClick={() => {
                  startAction(
                    run(async () => {
                      await client.probe(node.identity.environmentId, harness.harness);
                    }),
                    errorReporter(setIssue),
                  );
                }}
              >
                Check readiness
              </Button>
              {(hasText(harness.message) || hasText(harness.error)) && (
                <p className="text-muted w-full text-xs">
                  {nonEmptyText(harness.message) ?? harness.error}
                </p>
              )}
            </div>
          ))}
        {!state.harnesses.some((harness) => harness.available) && (
          <p className="text-muted text-xs leading-relaxed">
            {state.connected
              ? "No installed agent tools were found on this node."
              : "Agent information becomes available when this node is connected."}
          </p>
        )}
      </section>
      {node.local && (
        <Button variant="ghost" size="sm" className="mt-4" disabled={busy} onClick={onAccess}>
          <KeyRound size={14} />
          Connection access
        </Button>
      )}
      <p className="text-muted mt-5 text-xs leading-relaxed">
        Chats stay on their original node. Removing a connection preserves its execution, history,
        and files.
      </p>
      <footer className="mt-5 flex justify-end gap-2">
        <Button variant="ghost" disabled={busy} onClick={onClose}>
          Done
        </Button>
        <Button
          disabled={busy || !node.registered}
          onClick={() => {
            startAction(
              run(async () => {
                await client.select(node.identity.environmentId);
              }),
              errorReporter(setIssue),
            );
          }}
        >
          Open node
        </Button>
      </footer>
    </Frame>
  );
}
