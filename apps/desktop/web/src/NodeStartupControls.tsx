import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import type { ClientNode, NodeClient, NodeStartup } from "./nodes";

export function NodeStartupControls({ node, client, busy, run }: {
  node: ClientNode; client: NodeClient; busy: boolean; run: (action: () => Promise<unknown>) => Promise<void>;
}) {
  const [startup, setStartup] = useState<NodeStartup>();
  const [uncertain, setUncertain] = useState(false);
  const canInspect = node.local || node.connections.some((route) => route.transport.kind === "ssh" && !route.transport.socketPath);
  const inspect = () => run(async () => { setStartup(undefined); const observed = await client.startup(node); setStartup(observed); setUncertain(false); });
  const configure = (enabled: boolean, boot = false) => run(async () => {
    setUncertain(true);
    try { setStartup(await client.startup(node, enabled, boot)); setUncertain(false); }
    catch (error) { setStartup(undefined); throw new Error(`${error instanceof Error ? error.message : String(error)} Check startup again before retrying; configuration may have changed.`); }
  });
  return <section aria-label="Runtime startup" className="mb-5 border-t border-border pt-4">
    <h3 className="mb-2 text-sm font-medium">Runtime startup</h3>
    <p className="mb-3 text-xs leading-relaxed text-muted">Keep the runtime available when the app is closed. Changing future startup does not stop or restart active work.</p>
    {canInspect ? <>
      <Button variant="outline" size="sm" disabled={busy} onClick={() => void inspect()}>Check startup</Button>
      {uncertain && <p className="mt-3 text-xs text-muted" role="status">Check the current configuration before trying again.</p>}
      {startup && <div className="mt-3 grid gap-3">
        <Switch label={startup.trigger === "boot" ? "Start runtime when the computer boots" : "Start runtime when you sign in"}
          checked={startup.enabled === true} disabled={busy || !startup.available || startup.enabled === null}
          onChange={(enabled) => void configure(enabled)} />
        {(startup.reason || startup.notice) && <p className="text-xs leading-relaxed text-muted" role="status">{startup.reason || startup.notice}</p>}
        {startup.available && startup.canEnableBoot && !startup.bootReady && <Button variant="outline" size="sm" disabled={busy} onClick={() => void configure(true, true)}>Enable startup before sign-in and after logout</Button>}
        {startup.enabled && !startup.running && <p className="text-xs leading-relaxed text-muted">Automatic startup is enabled for the next sign-in or boot. The current runtime keeps running as it is; service supervision starts with the next service launch.</p>}
        {startup.running && <p className="text-xs text-muted">The operating system is supervising the runtime.</p>}
      </div>}
    </> : <p className="text-xs text-muted">Manage startup on this computer or through a saved SSH connection to its default runtime.</p>}
  </section>;
}
