import { createContext, useContext, useEffect, useLayoutEffect, useRef, useState, useSyncExternalStore, type ReactNode } from "react";
import { Mic, Square, Plug, X, LoaderCircle } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Dropdown } from "@/components/ui/dropdown";
import { ContributionRegistry, type ContributionContext, type ScopedClientApi, type UiSlot, type SurfaceEntry } from "./contributions";
import type { WorkspaceStore } from "./workspace";
import type { ExtensionInfo, Thread } from "./protocol";
import { readShortcuts, matchesShortcut } from "./shortcuts";
import { nativeClient } from "./clientTransport";
import { createDictationExtension, DictationSettings, DICTATION_EXTENSION_ID, type TranscriptionProvider } from "./dictation";
import { CustomExtensionView } from "./CustomExtensionView";

type Mount = (props: { api: ScopedClientApi }) => ReactNode;
export type ClientContribution = { extension: import("./contributions").ClientExtension; mounts?: Map<string, Mount> };
type HostContext = {
  registry: ContributionRegistry;
  mounts: Map<string, Mount>;
  bindComposer: (id: string, insert: (text: string, options?: { separateWords?: boolean }) => void) => () => void;
  store: WorkspaceStore;
  thread: Thread;
};
const Host = createContext<HostContext | null>(null);
export const useContributions = () => useContext(Host);
export function ContributionHost({ children, store, thread, ready, panelId, extensions, openSurface, clientContributions, transcriptionProvider }: {
  children: ReactNode; store: WorkspaceStore; thread: Thread; ready: boolean; panelId?: string;
  extensions: ExtensionInfo[]; openSurface: (kind: "screen" | "panel", id: string, context: ContributionContext) => void;
  clientContributions?: readonly ClientContribution[];
  transcriptionProvider?: TranscriptionProvider;
}) {
  const [registry] = useState(() => new ContributionRegistry());
  useSyncExternalStore(registry.subscribe, registry.getSnapshot);
  const runtimeRegistrations = useRef(new Map<string, { extension: ExtensionInfo; store: WorkspaceStore; dispose: () => void }>());
  const composer = useRef<{ id: string; insert: (text: string, options?: { separateWords?: boolean }) => void } | null>(null);
  const [mounts] = useState(() => new Map<string, Mount>([[`${DICTATION_EXTENSION_ID}/settings`, DictationSettings]]));
  const state = store.getSnapshot();
  const context: ContributionContext = {
    client: nativeClient() ? "desktop" : "browser", nodeId: state.node?.environmentId,
    conversationId: thread.id === "blank" ? undefined : thread.id,
    projectId: thread.projectId || undefined, worktreeId: thread.worktreeId || undefined,
    sessionId: thread.sessionId, connected: state.connected, composerReady: ready, panelId,
  };
  useLayoutEffect(() => {
    registry.configure({
      error: (error) => store.setError(error),
      openSurface: (kind, id, context) => openSurface(kind, id, context),
      insertText: (target, text, options) => {
        if (!composer.current || composer.current.id !== target.conversationId) throw new Error("Composer is unavailable.");
        composer.current.insert(text, options);
      },
    });
    registry.setContext(context);
  });
  useEffect(() => {
    const dispose = registry.registerBuiltin(createDictationExtension(registry, (error) => store.setError(error), transcriptionProvider));
    return dispose;
  }, [registry, store, transcriptionProvider]);
  useEffect(() => {
    const disposers: (() => void)[] = [];
    const addedMounts = new Map<string, Mount>();
    const cleanup = () => {
      for (const off of disposers) off();
      for (const [id, mount] of addedMounts) if (mounts.get(id) === mount) mounts.delete(id);
    };
    try {
      for (const contribution of clientContributions ?? []) {
        disposers.push(registry.register(contribution.extension));
        for (const [id, mount] of contribution.mounts ?? []) {
          if (!id.startsWith(`${contribution.extension.id}/`) || mounts.has(id)) throw new Error(`Invalid client renderer identity: ${id}`);
          mounts.set(id, mount); addedMounts.set(id, mount);
        }
      }
    } catch (error) { cleanup(); store.setError(error); }
    return cleanup;
  }, [registry, clientContributions, mounts, store]);
  useEffect(() => {
    for (const [id, registration] of runtimeRegistrations.current) {
      if (registration.store !== store || !extensions.some((extension) => extension === registration.extension && !!extension.ui)) {
        registration.dispose(); runtimeRegistrations.current.delete(id);
      }
    }
    for (const extension of extensions) {
      if (!extension.ui || runtimeRegistrations.current.has(extension.id)) continue;
      try {
        const handlers = Object.fromEntries((extension.ui.commands ?? []).map((command) => {
          if (!command.runtimeCommand || !extension.commands?.some((entry) => entry.name === command.runtimeCommand)) throw new Error("UI commands must name an advertised runtime command.");
          return [command.id, (api: ScopedClientApi) => store.invokeExtension(extension.id, command.runtimeCommand!, {
            projectId: api.context.worktreeId ?? api.context.projectId, projectRecordId: api.context.projectId, worktreeId: api.context.worktreeId, clientContext: api.context, sessionId: api.context.sessionId,
            conversationId: api.context.conversationId, messageId: api.context.messageId,
          })];
        }));
        for (const surface of [...(extension.ui.screens ?? []), ...(extension.ui.panels ?? [])]) {
          if (!surface.command || !extension.commands?.some((command) => command.name === surface.command)) throw new Error("UI surfaces must name an advertised runtime command.");
        }
        const dispose = registry.register({ id: extension.id, manifest: extension.ui, handlers });
        runtimeRegistrations.current.set(extension.id, { extension, store, dispose });
      } catch (error) { store.setError(`Extension ${extension.id}: ${String(error)}`); }
    }
  }, [extensions, registry, store]);
  useEffect(() => {
    const keydown = (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.repeat || !(event.metaKey || event.ctrlKey) || event.altKey) return;
      if (Object.values(readShortcuts()).some((binding) => matchesShortcut(event, binding))) return;
      const command = registry.commands().find((command) => {
        const parts = command.shortcut?.toLowerCase().split("+");
        return parts && parts.includes("shift") === event.shiftKey && parts.at(-1) === event.key.toLowerCase();
      });
      if (command) { event.preventDefault(); registry.run(command.key); }
    };
    window.addEventListener("keydown", keydown);
    return () => window.removeEventListener("keydown", keydown);
  }, [registry]);
  useEffect(() => () => {
    for (const registration of runtimeRegistrations.current.values()) registration.dispose();
    runtimeRegistrations.current.clear(); registry.dispose();
  }, [registry]);
  return <Host.Provider value={{ registry, mounts, store, thread, bindComposer: (id, insert) => {
    const binding = { id, insert }; composer.current = binding;
    return () => { if (composer.current === binding) composer.current = null; };
  } }}>{children}</Host.Provider>;
}
export function ContributionSlot({ name, context: extra, menu = false }: { name: UiSlot; context?: Partial<ContributionContext>; menu?: boolean }) {
  const host = useContributions();
  useSyncExternalStore(host?.registry.subscribe ?? noSubscribe, host?.registry.getSnapshot ?? zero);
  if (!host) return null;
  const context = { ...host.registry.getContext(), ...extra };
  const entries = host.registry.entries(name, context);
  if (!entries.length) return null;
  return <div data-contribution-slot={name} className={`contribution-slot ${menu ? "contribution-menu" : ""}`}>
    {entries.map((entry) => {
      if (entry.kind === "section") return <ContributionSettingsSection key={entry.key} entry={entry} />;
      if (entry.kind === "text") return <span key={entry.key} data-contribution-id={entry.key} role={name.endsWith("status") ? "status" : undefined} title={entry.title}>{entry.text ?? entry.title}</span>;
      const Icon = entry.icon ? contributionIcons[entry.icon] : undefined;
      const run = () => host.registry.run(`${entry.extensionId}/${entry.command}`, context);
      return menu ? <Dropdown.Item key={entry.key} id={entry.key} textValue={entry.title} isDisabled={!entry.enabled} onAction={run}>{entry.title}</Dropdown.Item>
        : <Button key={entry.key} data-contribution-id={entry.key} type="button" variant="ghost" size="sm" disabled={!entry.enabled} aria-label={entry.title} title={entry.title} onClick={run}>
          {Icon ? <Icon size={16} className={entry.icon === "spinner" ? "animate-spin motion-reduce:animate-none" : undefined} /> : entry.title}
        </Button>;
    })}
  </div>;
}
const contributionIcons = { microphone: Mic, stop: Square, spinner: LoaderCircle, extension: Plug };
const noSubscribe = () => () => {};
const zero = () => 0;
function ContributionSettingsSection({ entry }: { entry: import("./contributions").SlotEntry }) {
  const host = useContributions()!;
  return <section data-contribution-id={entry.key} className="settings-section"><h2>{entry.title}</h2>
    {entry.text && <p>{entry.text}</p>}
    {host.mounts.has(entry.key) && <ClientMount key={`${entry.key}:${host.registry.getContext().nodeId}:${host.registry.getContext().conversationId}:${host.registry.getContext().projectId}:${host.registry.getContext().worktreeId}:${host.registry.getContext().sessionId}`} id={entry.key} owner={entry.extensionId} />}
    {entry.command && <Button variant="secondary" onClick={() => host.registry.run(`${entry.extensionId}/${entry.command}`)} disabled={!entry.enabled}>Open {entry.title}</Button>}
  </section>;
}
function ClientMount({ id, owner, context }: { id: string; owner: string; context?: ContributionContext }) {
  const host = useContributions()!;
  const [api, setApi] = useState<ScopedClientApi>();
  useEffect(() => { const scoped = host.registry.api(owner, context); setApi(scoped); return () => scoped.dispose(); }, [host.registry, owner]);
  const Mount = host.mounts.get(id)!;
  return api ? <Mount api={api} /> : null;
}
export function ContributionSurface({ kind, id, onClose, context: supplied }: { kind: "screen" | "panel"; id: string; onClose: () => void; context?: ContributionContext }) {
  const host = useContributions()!;
  useSyncExternalStore(host.registry.subscribe, host.registry.getSnapshot);
  const entry = host.registry.surfaces(kind).find((surface) => surface.key === id);
  const context = { ...(supplied ?? host.registry.getContext()), composerReady: kind === "screen" ? false : (supplied?.composerReady ?? host.registry.getContext().composerReady) };
  useEffect(() => {
    if (!entry) { queueMicrotask(() => { if (!host.registry.surfaces(kind).some((surface) => surface.key === id)) onClose(); }); return; }
    host.registry.emit({ type: "surface.opened", kind, id, context });
    return () => host.registry.emit({ type: "surface.closed", kind, id, context });
  }, [id, kind, !!entry, context.nodeId, context.conversationId, context.sessionId, context.messageId, context.projectId, context.worktreeId]);
  if (!entry) return null;
  const runtime = host.store.getSnapshot().extensions.find((extension) => extension.id === entry.extensionId);
  return <section className={`contribution-surface ${kind}`} aria-label={entry.title}>
    <div className="detail-heading"><h2>{entry.title}</h2><Button variant="ghost" size="icon-sm" aria-label={`Close ${entry.title}`} onClick={onClose}><X size={16} /></Button></div>
    {kind === "panel" && <ContributionSlot name="panel.toolbar" context={{ ...context, panelId: id }} />}
    {host.mounts.has(id) ? <ClientMount key={`${id}:${context.nodeId}:${context.conversationId}:${context.projectId}:${context.worktreeId}:${context.sessionId}:${context.messageId}`} id={id} owner={entry.extensionId} context={context} /> : runtime && entry.command
      ? <CustomExtensionView store={host.store} thread={host.store.getSnapshot().threads.find((thread) => thread.id === context.conversationId) ?? { ...host.thread, id: context.conversationId ?? "blank", projectId: context.projectId ?? "", worktreeId: context.worktreeId ?? "", sessionId: context.sessionId }} context={context} extension={runtime} view={{ ...entry, command: entry.command, kind: "web" }} />
      : <p className="panel-note">This client has no renderer for this surface.</p>}
    {kind === "panel" && <ContributionSlot name="panel.status" context={{ ...context, panelId: id }} />}
  </section>;
}
export function ContributionNavigation({ kind }: { kind: "screen" | "panel" }) {
  const host = useContributions();
  useSyncExternalStore(host?.registry.subscribe ?? noSubscribe, host?.registry.getSnapshot ?? zero);
  if (!host) return null;
  const surfaces = host.registry.surfaces(kind);
  if (!surfaces.length) return null;
  return <SurfaceNavigation entries={surfaces} kind={kind} />;
}
function SurfaceNavigation({ entries, kind }: { entries: SurfaceEntry[]; kind: "screen" | "panel" }) {
  const host = useContributions()!;
  // Navigation uses the same scoped operation exposed to commands and isolated views.
  const open = (entry: SurfaceEntry) => {
    try { const api = host.registry.api(entry.extensionId); try { kind === "screen" ? api.openScreen(entry.id) : api.openPanel(entry.id); } finally { api.dispose(); } }
    catch (error) { host.store.setError(error); }
  };
  return <Dropdown><Button variant="ghost" size="sm" aria-label={`Extension ${kind}s`}><Plug size={14} />{kind === "screen" ? "Screens" : "Panels"}</Button>
    <Dropdown.Popover><Dropdown.Menu aria-label={`Extension ${kind}s`}>{entries.map((entry) => <Dropdown.Item key={entry.key} id={entry.key} onAction={() => open(entry)}>{entry.title}</Dropdown.Item>)}</Dropdown.Menu></Dropdown.Popover>
  </Dropdown>;
}
