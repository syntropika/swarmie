import { useId, useState } from "react";
import { KeyRound, Monitor, MoreHorizontal, RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Dropdown } from "@/components/ui/dropdown";
import { Switch } from "@/components/ui/switch";
import { Tooltip, TooltipContent } from "@/components/ui/tooltip";
import { AddNodeDialog, LocalAccessDialog, NodeDetailsDialog, routeAddress } from "./NodeDialogs";
import type { NodeClient, NodesState } from "./nodes";

const views = [
  { id: "computer", label: "Control this computer" },
  { id: "devices", label: "Control other devices" },
] as const;
type View = typeof views[number]["id"] | "ssh";

export function NodeSettings({ client, nodes }: { client: NodeClient; nodes: NodesState }) {
  const id = useId();
  const [view, setView] = useState<View>("computer");
  const [dialog, setDialog] = useState<{ kind: "add" | "access" } | { kind: "details"; id: string }>();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const run = async (action: () => Promise<unknown>) => {
    setBusy(true); setError("");
    try { await action(); } catch (error) { setError(error instanceof Error ? error.message : String(error)); } finally { setBusy(false); }
  };
  const local = client.localSnapshot();
  const detail = dialog?.kind === "details" ? nodes.nodes.find((node) => node.identity.environmentId === dialog.id) : undefined;
  const tabs = nodes.nodes.some((node) => !node.local && node.connections.some((route) => route.transport.kind === "ssh"))
    ? [...views, { id: "ssh" as const, label: "SSH" }] : [...views];
  const activeView = tabs.some((tab) => tab.id === view) ? view : "devices";
  const visible = nodes.nodes.filter((node) => activeView === "computer" ? node.local
    : !node.local && (activeView !== "ssh" || node.connections.some((route) => route.transport.kind === "ssh")));
  const remoteView = activeView !== "computer";
  return <div>
    <div role="tablist" aria-label="Connections view" className="mb-7 flex w-fit max-w-full gap-1 overflow-x-auto rounded-lg bg-control p-1">
      {tabs.map((tab, index) => <Button key={tab.id} id={`${id}-${tab.id}`} role="tab" aria-selected={activeView === tab.id} aria-controls={`${id}-panel`} tabIndex={activeView === tab.id ? 0 : -1}
        variant={activeView === tab.id ? "secondary" : "ghost"} size="sm" className="shrink-0" onClick={() => setView(tab.id)} onKeyDown={(event) => {
          const next = event.key === "ArrowRight" ? (index + 1) % tabs.length : event.key === "ArrowLeft" ? (index + tabs.length - 1) % tabs.length : event.key === "Home" ? 0 : event.key === "End" ? tabs.length - 1 : undefined;
          if (next !== undefined) { event.preventDefault(); setView(tabs[next].id); document.getElementById(`${id}-${tabs[next].id}`)?.focus(); }
        }}>{tab.label}</Button>)}
    </div>
    <section id={`${id}-panel`} role="tabpanel" aria-labelledby={`${id}-${activeView}`}>
      <div className="mb-4 flex items-center justify-between gap-3">
        <h2 className="text-sm font-medium">{activeView === "computer" ? "This computer" : activeView === "ssh" ? "SSH connections from this computer" : "Devices you can control from this computer"}</h2>
        {remoteView && <div className="flex shrink-0 items-center gap-1">
          <Tooltip><Button variant="ghost" size="icon-sm" aria-label="Refresh nodes" disabled={busy || nodes.loading || !local.connected} onClick={() => void run(() => client.refresh())}><RefreshCw size={15} className={nodes.loading ? "animate-spin motion-reduce:animate-none" : ""} /></Button><TooltipContent>Refresh</TooltipContent></Tooltip>
          {!!visible.length && <Button variant="secondary" size="sm" disabled={busy || !local.connected} onClick={() => setDialog({ kind: "add" })}>Add</Button>}
        </div>}
      </div>
      {(error || nodes.error) && <p className="mb-4 break-words text-sm text-danger" role="alert">{error || nodes.error}</p>}
      <div role="list" aria-label={remoteView ? "Remote nodes" : "Local node"} className="divide-y divide-border">
        {visible.map((node) => {
          const status = client.status(node);
          const connected = status === "Connected";
          const description = node.local ? "Connects automatically" : node.connections.length === 1 ? routeAddress(node.connections[0]) : `${node.connections.length} saved connections`;
          return <div role="listitem" key={node.identity.environmentId} className="flex items-center gap-3 py-4">
            <Monitor size={19} className="shrink-0 text-muted" />
            <div className="min-w-0 flex-1"><p className="truncate text-sm font-medium" title={node.displayName}>{node.displayName}</p><p className="mt-1 truncate text-xs text-muted" title={description}>{description}</p><p className="mt-1 text-xs text-muted">{status}</p></div>
            {!node.local && <Switch label={`Connect ${node.displayName}`} checked={connected} disabled={busy || !node.registered} onChange={(value) => value ? void run(() => client.connect(node.identity.environmentId)) : client.disconnect(node.identity.environmentId)} hideLabel />}
            <Dropdown>
              <Button variant="ghost" size="icon-sm" aria-label={`${node.displayName} actions`} disabled={busy}><MoreHorizontal size={17} /></Button>
              <Dropdown.Popover placement="bottom end"><Dropdown.Menu aria-label={`${node.displayName} actions`}>
                <Dropdown.Item id="details" onAction={() => setDialog({ kind: "details", id: node.identity.environmentId })}>Details</Dropdown.Item>
                <Dropdown.Item id="open" isDisabled={!node.registered} onAction={() => void run(() => client.select(node.identity.environmentId))}>Open node</Dropdown.Item>
              </Dropdown.Menu></Dropdown.Popover>
            </Dropdown>
          </div>;
        })}
        {!visible.length && <div className="flex flex-col items-center gap-3 py-8 text-center">
          <Monitor size={24} className="text-muted" />
          <p className="text-sm text-muted">{remoteView ? "No devices added yet" : local.error || "Connecting automatically…"}</p>
          {remoteView ? <Button variant="secondary" disabled={busy || !local.connected} onClick={() => setDialog({ kind: "add" })}>Add</Button>
            : /token/i.test(local.error) && <Button variant="secondary" onClick={() => setDialog({ kind: "access" })}><KeyRound size={14} />Connection access</Button>}
        </div>}
      </div>
      {activeView === "computer" && <p className="mt-4 text-xs leading-relaxed text-muted">The app connects to this computer automatically and retries after a temporary connection loss. Remote access from other devices is not configured here yet.</p>}
      {remoteView && <p className="mt-4 text-xs leading-relaxed text-muted">Add a running runtime by address or through configured SSH access. Existing chats keep their original node.</p>}
    </section>
    {dialog?.kind === "add" && <AddNodeDialog client={client} onClose={() => setDialog(undefined)} />}
    {dialog?.kind === "access" && <LocalAccessDialog client={client} onClose={() => setDialog(undefined)} />}
    {detail && <NodeDetailsDialog node={detail} client={client} onClose={() => setDialog(undefined)} onAccess={() => setDialog({ kind: "access" })} />}
  </div>;
}
