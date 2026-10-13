import { useState, type ReactNode } from "react";
import { KeyRound, Monitor, Trash2, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Dialog } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { HarnessIcon } from "./HarnessSelect";
import type { ClientNode, NodeClient, NodeRoute, SshReadiness } from "./nodes";

export function routeAddress(route: NodeRoute): string {
  if (route.transport.kind === "ssh") return `${route.transport.host ?? "SSH connection"} · ${route.transport.socketPath || "Default runtime"}`;
  return route.transport.path ?? route.transport.url ?? "Saved connection";
}
function Frame({ title, busy, onClose, children }: { title: string; busy: boolean; onClose: () => void; children: ReactNode }) {
  return <Dialog label={title} onClose={onClose} busy={busy} className="max-w-[480px] p-6">
    <header className="mb-5 flex items-center justify-between gap-3">
      <h2 className="min-w-0 truncate text-lg font-semibold" title={title}>{title}</h2>
      <Button variant="ghost" size="icon-sm" aria-label="Close node dialog" disabled={busy} onClick={onClose}><X size={16} /></Button>
    </header>
    {children}
  </Dialog>;
}
const errorText = (error: unknown) => error instanceof Error ? error.message : String(error);

function sshReadinessText(readiness: SshReadiness): string {
  switch (readiness.status) {
    case "ready": return `Runtime ready${readiness.version ? ` · ${readiness.version}` : ""}`;
    case "not-installed": return "Install Swarmie runtime on the remote computer, then check again.";
    case "ssh-client-unavailable": return "Install OpenSSH on this computer, then check again.";
    case "ssh-unavailable": return "Check the SSH host, network, host trust and configured authentication, then check again.";
    case "invalid-installation": return "Check the remote Swarmie installation and shell startup output, then check again.";
    case "incompatible-runtime": return "The remote runtime could not be verified. Update its Swarmie installation, then check again.";
    case "runtime-unavailable": return "Check that the runtime service is running on the remote computer and the socket path is correct, then check again.";
  }
}

export function AddNodeDialog({ client, onClose, initialMethod = "direct" }: { client: NodeClient; onClose: () => void; initialMethod?: "direct" | "ssh" }) {
  const [method, setMethod] = useState<"direct" | "ssh">(initialMethod);
  const [name, setName] = useState("");
  const [address, setAddress] = useState("");
  const [host, setHost] = useState("");
  const [socket, setSocket] = useState("");
  const [token, setToken] = useState("");
  const [readiness, setReadiness] = useState<SshReadiness>();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const ssh = method === "ssh";
  const check = async () => {
    setBusy(true); setError(""); setReadiness(undefined);
    try { setReadiness(await client.inspectSsh(host.trim(), socket.trim())); }
    catch (error) { setError(errorText(error)); }
    finally { setBusy(false); }
  };
  return <Frame title="Add node" busy={busy} onClose={onClose}>
    <p className="mb-5 text-sm leading-relaxed text-muted">Connect a computer with a running runtime.</p>
    <form className="grid gap-4" aria-busy={busy} onSubmit={(event) => {
      event.preventDefault();
      if (busy || (ssh && (!readiness?.ready || !readiness.runtimeIdentity))) return;
      setBusy(true); setError("");
      const pair = ssh ? client.pairSsh(name.trim(), host.trim(), socket.trim(), readiness!.runtimeIdentity!) : client.pair(name.trim(), address.trim(), token);
      void pair.then(onClose).catch((error) => { setError(errorText(error)); if (ssh) setReadiness(undefined); }).finally(() => setBusy(false));
    }}>
      <label className="grid gap-1.5 text-xs font-medium">Connection name<Input autoFocus placeholder="Work computer" required disabled={busy} value={name} onChange={(event) => setName(event.target.value)} /></label>
      <label className="grid gap-1.5 text-xs font-medium">Connection method<select className="h-9 rounded-md border border-border bg-background px-3 text-sm" disabled={busy} value={method} onChange={(event) => { setMethod(event.target.value as "direct" | "ssh"); setReadiness(undefined); setError(""); setToken(""); }}><option value="direct">Address</option><option value="ssh">SSH</option></select></label>
      {ssh ? <>
        <label className="grid gap-1.5 text-xs font-medium">SSH host<Input placeholder="user@server or SSH alias" required disabled={busy} value={host} onChange={(event) => { setHost(event.target.value); setReadiness(undefined); }} /></label>
        <details className="text-xs"><summary className="cursor-pointer text-muted">Advanced</summary><label className="mt-3 grid gap-1.5 text-xs font-medium">Runtime socket path <span className="font-normal text-muted">Optional override</span><Input aria-label="Runtime socket path" placeholder="Use the default runtime" disabled={busy} value={socket} onChange={(event) => { setSocket(event.target.value); setReadiness(undefined); }} /></label></details>
        <p className="text-xs leading-relaxed text-muted">Use the SSH access configured on this computer. No account is required. Checking leaves the remote runtime unchanged.</p>
        <Button type="button" variant="outline" disabled={busy || !host.trim()} onClick={() => void check()}>{busy ? "Checking…" : "Check connection"}</Button>
        {readiness && <p className="text-sm text-muted" role="status">{sshReadinessText(readiness)}</p>}
      </> : <>
        <label className="grid gap-1.5 text-xs font-medium">Address<Input placeholder="http://localhost:7490" required disabled={busy} value={address} onChange={(event) => setAddress(event.target.value)} /></label>
        <label className="grid gap-1.5 text-xs font-medium">Access token <span className="font-normal text-muted">Optional</span><Input type="password" aria-label="Access token" autoComplete="off" disabled={busy} value={token} onChange={(event) => setToken(event.target.value)} /></label>
      </>}
      {error && <p className="text-sm text-danger" role="alert">{error}</p>}
      <p className="text-xs leading-relaxed text-muted">The connection is verified before it is saved. Credentials stay on this computer.</p>
      <footer className="mt-2 flex justify-end gap-2"><Button variant="ghost" disabled={busy} onClick={onClose}>Cancel</Button><Button type="submit" disabled={busy || !name.trim() || (ssh ? !readiness?.ready || !readiness.runtimeIdentity : !address.trim())}>{busy ? "Connecting…" : "Add node"}</Button></footer>
    </form>
  </Frame>;
}

export function LocalAccessDialog({ client, onClose }: { client: NodeClient; onClose: () => void }) {
  const [token, setToken] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  return <Frame title="Local connection access" busy={busy} onClose={onClose}>
    <p className="mb-5 text-sm leading-relaxed text-muted">This computer connects automatically. If its runtime requires a token, enter it here. The token stays in memory.</p>
    <form className="grid gap-4" onSubmit={(event) => {
      event.preventDefault(); if (busy) return;
      setBusy(true); setError("");
      void client.reconnect(token).then(() => {
        if (!client.localSnapshot().connected) throw new Error(client.localSnapshot().error || "The local connection is not available yet.");
        onClose();
      }).catch((error) => setError(errorText(error))).finally(() => setBusy(false));
    }}>
      <label className="grid gap-1.5 text-xs font-medium">Local access token<Input autoFocus type="password" autoComplete="off" value={token} disabled={busy} onChange={(event) => setToken(event.target.value)} /></label>
      {error && <p className="text-sm text-danger" role="alert">{error}</p>}
      <footer className="mt-2 flex justify-end gap-2"><Button variant="ghost" disabled={busy} onClick={onClose}>Cancel</Button><Button type="submit" disabled={busy}>{busy ? "Verifying…" : "Save token"}</Button></footer>
    </form>
  </Frame>;
}

export function NodeDetailsDialog({ node, client, onClose, onAccess }: { node: ClientNode; client: NodeClient; onClose: () => void; onAccess: () => void }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const state = client.nodeSnapshot(node);
  const status = client.status(node);
  const run = async (action: () => Promise<unknown>) => {
    setBusy(true); setError("");
    try { await action(); } catch (error) { setError(errorText(error)); } finally { setBusy(false); }
  };
  const lastSeen = node.lastVerifiedAtMs > 0 ? new Date(node.lastVerifiedAtMs).toLocaleString() : "Not yet verified";
  return <Frame title={node.displayName} busy={busy} onClose={onClose}>
    <div className="mb-5 flex items-center gap-3"><Monitor size={20} className="text-muted" /><div className="min-w-0"><p className="text-sm">{status}</p><p className="text-xs text-muted">{node.local ? "Connects automatically on this computer" : "Remote node"}</p></div></div>
    {(error || state.error) && <p className="mb-4 break-words text-sm text-danger" role="alert">{error || state.error}</p>}
    <dl className="mb-5 grid grid-cols-[auto_minmax(0,1fr)] gap-x-5 gap-y-2 text-xs"><dt className="text-muted">Last verified</dt><dd className="text-right">{lastSeen}</dd><dt className="text-muted">Connection</dt><dd className="text-right">{node.local ? "Automatic" : `${node.connections.length} saved route${node.connections.length === 1 ? "" : "s"}`}</dd></dl>
    {node.connections.length > 0 && <section aria-label="Saved connections" className="mb-5 border-t border-border pt-4">
      <h3 className="mb-2 text-sm font-medium">Connections</h3>
      <div className="divide-y divide-border">{node.connections.map((route) => <div key={route.name} className="flex items-center gap-3 py-3">
        <div className="min-w-0 flex-1"><p className="truncate text-sm">{route.name}</p><p className="break-all text-xs text-muted">{routeAddress(route)}</p></div>
        <Button variant="ghost" size="icon-sm" aria-label={`Remove connection ${route.name}`} disabled={busy} onClick={() => void run(() => client.removeRoute(route.name, route.revision))}><Trash2 size={15} /></Button>
      </div>)}</div>
    </section>}
    <section aria-label="Node harnesses" className="border-t border-border pt-4">
      <h3 className="mb-2 text-sm font-medium">Available harnesses</h3>
      {state.harnesses.filter((harness) => harness.available).map((harness) => <div key={harness.harness} className="flex flex-wrap items-center gap-2 py-2">
        <HarnessIcon harness={harness.harness} /><span className="text-sm">{harness.harness}</span><span className="text-xs text-muted">{harness.authState ?? "Installed"}</span>
        <Button className="ml-auto" variant="ghost" size="sm" disabled={busy || !state.connected} onClick={() => void run(() => client.probe(node.identity.environmentId, harness.harness))}>Check readiness</Button>
        {(harness.message || harness.error) && <p className="w-full text-xs text-muted">{harness.message || harness.error}</p>}
      </div>)}
      {!state.harnesses.some((harness) => harness.available) && <p className="text-xs leading-relaxed text-muted">{state.connected ? "No installed harnesses were found on this node." : "Harness information becomes available when this node is connected."}</p>}
    </section>
    {node.local && <Button variant="ghost" size="sm" className="mt-4" disabled={busy} onClick={onAccess}><KeyRound size={14} />Connection access</Button>}
    <p className="mt-5 text-xs leading-relaxed text-muted">Chats stay on their original node. Removing a connection preserves its execution, history, and files.</p>
    <footer className="mt-5 flex justify-end gap-2"><Button variant="ghost" disabled={busy} onClick={onClose}>Done</Button><Button disabled={busy || !node.registered} onClick={() => void run(() => client.select(node.identity.environmentId))}>Open node</Button></footer>
  </Frame>;
}
