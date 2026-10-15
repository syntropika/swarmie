import type {
  UiSlot,
  UiCondition,
  UiContribution,
  UiCommand,
  UiSurface,
  UiClient,
} from "@swarmie/contracts";
import type {
  ContributionContext,
  ContributionEvent,
  ScopedClientApi,
  ClientExtension,
} from "@swarmie/contracts/ui-client";

import { startAction } from "~/src/action-boundary";
import { sortedCopy } from "~/src/collection-order";
import { hasText } from "~/src/value-presence";

export type { UiManifest, UiSlot, UiContribution, UiSurface } from "@swarmie/contracts";

export const slotCatalog: Record<UiSlot, readonly UiContribution["kind"][]> = {
  "sidebar.primary": ["action"],
  "sidebar.footer": ["action", "text"],
  "app.header.actions": ["action"],
  "conversation.header.actions": ["action"],
  "composer.actions": ["action"],
  "composer.context": ["action", "text"],
  "composer.status": ["text"],
  "message.actions": ["action"],
  "message.details": ["text"],
  "project.actions": ["action"],
  "project.contextMenu": ["action"],
  "conversation.contextMenu": ["action"],
  "panel.toolbar": ["action"],
  "panel.status": ["text"],
  "settings.sections": ["section"],
};

export type {
  ContributionContext,
  ContributionEvent,
  ScopedClientApi,
  ClientExtension,
  CommandHandler,
} from "@swarmie/contracts/ui-client";
type Registered = ClientExtension & { controller: AbortController; cleanup?: () => void };
type Host = {
  insertText: (
    context: ContributionContext,
    text: string,
    options?: { separateWords?: boolean },
  ) => void;
  openSurface: (kind: "screen" | "panel", id: string, context: ContributionContext) => void;
  error: (error: unknown) => void;
};
export type SlotEntry = UiContribution & { extensionId: string; key: string; enabled: boolean };
export type SurfaceEntry = UiSurface & { extensionId: string; key: string };
const localId = /^[a-zA-Z0-9][a-zA-Z0-9._-]{0,95}$/u;
const scopeKey = (context: ContributionContext) =>
  JSON.stringify([
    context.nodeId,
    context.conversationId,
    context.projectId,
    context.worktreeId,
    context.sessionId,
  ]);
export function conditionsMatch(
  conditions: UiCondition[] | undefined,
  context: ContributionContext,
) {
  const values: Record<UiCondition, boolean> = {
    connected: context.connected,
    conversation: hasText(context.conversationId),
    project: hasText(context.projectId),
    message: hasText(context.messageId),
    panel: hasText(context.panelId),
    composerReady: context.composerReady,
  };
  return (conditions ?? []).every((condition) => values[condition]);
}
function validate(extension: ClientExtension, builtin = false) {
  const { manifest: m, id } = extension;
  const validId = builtin
    ? id.startsWith("@swarmie.") && localId.test(id.slice("@swarmie.".length))
    : localId.test(id);
  if (!validId || m.version !== 1) {
    throw new Error("Unsupported extension identity or contribution version.");
  }
  const unique = (values: { id: string }[]) => {
    const ids = new Set<string>();
    for (const value of values) {
      if (!localId.test(value.id) || ids.has(value.id)) {
        throw new Error(`Invalid or duplicate contribution identity: ${value.id}`);
      }
      ids.add(value.id);
    }
  };
  unique(m.commands ?? []);
  unique(m.slots ?? []);
  unique([...(m.screens ?? []), ...(m.panels ?? [])]);
  const commands = validateCommands(extension);

  for (const slot of m.slots ?? []) {
    if (!slotCatalog[slot.slot]?.includes(slot.kind) || !Number.isFinite(slot.order ?? 0)) {
      throw new Error(`Unsupported contribution type in ${slot.slot}`);
    }
    if (
      (slot.kind === "action" && !hasText(slot.command)) ||
      (hasText(slot.command) && !commands.has(slot.command))
    ) {
      throw new Error(`Unknown slot command: ${slot.id}`);
    }
  }
}

/** One registry per client workspace. No runtime HTML or runtime listener executes in the host. */
export class ContributionRegistry {
  private readonly extensions = new Map<string, Registered>();
  private readonly listeners = new Set<() => void>();
  private readonly events = new Set<{
    owner: string;
    type: ContributionEvent["type"];
    listener: (event: ContributionEvent) => void;
  }>();
  private readonly scopes = new Set<AbortController>();
  private readonly states = new Map<
    string,
    Partial<Pick<SlotEntry, "title" | "text" | "enabled" | "icon">>
  >();
  private readonly commandEnabled = new Map<string, boolean>();
  private revision = 0;
  private context: ContributionContext = {
    client: "browser",
    connected: false,
    composerReady: false,
  };
  private epoch = 0;
  private host: Host = {
    insertText: () => {
      throw new Error("Composer is unavailable.");
    },
    openSurface: () => {
      throw new Error("Contribution host is unavailable.");
    },
    error: (error) => {
      throw error;
    },
  };
  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };
  getSnapshot = () => this.revision;
  getContext = () => this.context;
  getLifetime(id: string) {
    return this.extensions.get(id)?.controller.signal;
  }
  configure(host: Host) {
    this.host = host;
  }
  private changed() {
    this.revision += 1;
    for (const listener of this.listeners) {
      listener();
    }
  }
  setContext(next: ContributionContext) {
    if (JSON.stringify(next) === JSON.stringify(this.context)) {
      return;
    }
    if (scopeKey(next) !== scopeKey(this.context)) {
      this.epoch += 1;
      for (const scope of this.scopes) {
        scope.abort();
      }
      this.scopes.clear();
    }
    this.context = Object.freeze({ ...next });
    this.emit({ type: "context.changed", context: this.context });
    this.changed();
  }
  register(extension: ClientExtension) {
    return this.add(extension, false);
  }
  /** Host-owned IDs are outside the namespace accepted by public registrations. */
  registerBuiltin(extension: ClientExtension) {
    return this.add(extension, true);
  }
  private add(extension: ClientExtension, builtin: boolean) {
    validate(extension, builtin);
    if (this.extensions.has(extension.id)) {
      throw new Error(`Extension already registered: ${extension.id}`);
    }
    const registered: Registered = {
      ...extension,
      manifest: structuredClone(extension.manifest),
      controller: new AbortController(),
    };
    const shortcuts = new Set(
      [...this.extensions.values()].flatMap((entry) =>
        (entry.manifest.commands ?? []).flatMap((command) =>
          hasText(command.shortcut) ? [command.shortcut.toLowerCase()] : [],
        ),
      ),
    );
    for (const command of registered.manifest.commands ?? []) {
      if (!hasText(command.shortcut)) {
        continue;
      }
      const shortcut = command.shortcut.toLowerCase();
      if (shortcuts.has(shortcut)) {
        throw new Error(`Shortcut already registered: ${command.shortcut}`);
      }
      shortcuts.add(shortcut);
    }
    this.extensions.set(extension.id, registered);
    try {
      // Activation supplies recurring events and returns a lifetime disposer, not a completion listener.
      registered.cleanup =
        extension.activate?.({
          signal: registered.controller.signal,
          on: (type, listener) => this.subscribeEvent(extension.id, type, listener),
        }) ?? undefined;
    } catch (error) {
      this.remove(extension.id);
      throw error;
    }
    this.changed();
    return () => {
      if (this.extensions.get(extension.id) === registered) {
        this.remove(extension.id);
      }
    };
  }
  private remove(id: string) {
    const extension = this.extensions.get(id);
    if (!extension) {
      return;
    }
    this.extensions.delete(id);
    extension.controller.abort();
    try {
      extension.cleanup?.();
    } catch (error) {
      this.host.error(error);
    }
    for (const event of this.events) {
      if (event.owner === id) {
        this.events.delete(event);
      }
    }
    for (const key of this.states.keys()) {
      if (key.startsWith(`${id}/`)) {
        this.states.delete(key);
      }
    }
    for (const key of this.commandEnabled.keys()) {
      if (key.startsWith(`${id}/`)) {
        this.commandEnabled.delete(key);
      }
    }
    this.changed();
  }
  dispose() {
    for (const id of this.extensions.keys()) {
      this.remove(id);
    }
    for (const scope of this.scopes) {
      scope.abort();
    }
    this.scopes.clear();
    this.events.clear();
  }
  update(
    extensionId: string,
    contributionId: string,
    state: Partial<Pick<SlotEntry, "title" | "text" | "enabled" | "icon">>,
  ) {
    if (
      !(
        this.extensions
          .get(extensionId)
          ?.manifest.slots?.some((slot) => slot.id === contributionId) === true
      )
    ) {
      return;
    }
    this.states.set(`${extensionId}/${contributionId}`, state);
    this.changed();
  }
  entries(slot: UiSlot, context: ContributionContext = this.context): SlotEntry[] {
    return sortedCopy(
      [...this.extensions.values()].flatMap((extension) =>
        (extension.manifest.slots ?? [])
          .filter(
            (entry) =>
              entry.slot === slot &&
              supports(entry.clients, context) &&
              conditionsMatch(entry.when, context),
          )
          .map((entry) => ({
            ...entry,
            clients: entry.clients ?? [],
            when: entry.when ?? [],
            enabledWhen: entry.enabledWhen ?? [],
            order: entry.order ?? 0,
            extensionId: extension.id,
            key: `${extension.id}/${entry.id}`,
            ...this.states.get(`${extension.id}/${entry.id}`),
            enabled:
              this.states.get(`${extension.id}/${entry.id}`)?.enabled !== false &&
              conditionsMatch(entry.enabledWhen, context) &&
              (!hasText(entry.command) ||
                this.canExecute(`${extension.id}/${entry.command}`, context)),
          })),
      ),
      (a, b) => (a.order ?? 0) - (b.order ?? 0) || a.key.localeCompare(b.key, "en"),
    );
  }

  surfaces(kind: "screen" | "panel", context = this.context): SurfaceEntry[] {
    return [...this.extensions.values()].flatMap((extension) =>
      (extension.manifest[kind === "screen" ? "screens" : "panels"] ?? [])
        .filter((surface) => supports(surface.clients, context))
        .map((surface) => ({
          ...surface,
          clients: surface.clients ?? [],
          extensionId: extension.id,
          key: `${extension.id}/${surface.id}`,
        })),
    );
  }
  setCommandEnabled(key: string, enabled: boolean) {
    if (!this.extensions.get(key.split("/")[0]) || this.commandEnabled.get(key) === enabled) {
      return;
    }
    this.commandEnabled.set(key, enabled);
    this.changed();
  }
  commands(context = this.context): (UiCommand & { key: string })[] {
    return [...this.extensions.values()].flatMap((extension) =>
      (extension.manifest.commands ?? [])
        .filter((command) => this.canExecute(`${extension.id}/${command.id}`, context))
        .map((command) => ({
          ...command,
          when: command.when ?? [],
          key: `${extension.id}/${command.id}`,
        })),
    );
  }
  canExecute(key: string, context = this.context) {
    const [owner, id] = key.split("/");
    const extension = this.extensions.get(owner);
    const command = extension?.manifest.commands?.find((entry) => entry.id === id);
    return (
      !!command &&
      this.commandEnabled.get(key) !== false &&
      !(extension?.controller.signal.aborted === true) &&
      conditionsMatch(command.when, context)
    );
  }
  api(owner: string, context = this.context): ScopedClientApi {
    const extension = this.extensions.get(owner);
    if (!extension) {
      throw new Error("Extension is inactive.");
    }
    const { epoch } = this;
    const controller = new AbortController();
    this.scopes.add(controller);
    const abort = () => {
      controller.abort();
      this.scopes.delete(controller);
    };
    extension.controller.signal.addEventListener("abort", abort, { once: true });
    controller.signal.addEventListener(
      "abort",
      () => {
        extension.controller.signal.removeEventListener("abort", abort);
      },
      { once: true },
    );
    const check = () => {
      if (
        controller.signal.aborted ||
        epoch !== this.epoch ||
        this.extensions.get(owner) !== extension
      ) {
        throw new Error("Contribution context expired.");
      }
    };
    const open = (kind: "screen" | "panel", id: string) => {
      check();
      const key = `${owner}/${id}`;
      if (!this.surfaces(kind, context).some((surface) => surface.key === key)) {
        throw new Error("Unknown extension surface.");
      }
      this.host.openSurface(kind, key, context);
    };
    return {
      signal: controller.signal,
      dispose: abort,
      context: Object.freeze({ ...context }),
      insertText: (text, options) => {
        check();
        if (
          !context.composerReady ||
          context.conversationId !== this.context.conversationId ||
          !this.context.composerReady
        ) {
          throw new Error("Composer is unavailable.");
        }
        if (typeof text !== "string" || new TextEncoder().encode(text).length > 1024 * 1024) {
          throw new Error("Use composer text under 1 MB.");
        }
        if (options?.separateWords === true) {
          this.host.insertText(context, text, { separateWords: true });
        } else {
          this.host.insertText(context, text);
        }
      },
      openScreen: (id) => {
        open("screen", id);
      },
      openPanel: (id) => {
        open("panel", id);
      },
      on: (type, listener) => {
        check();
        // Subscription listeners repeat until the scoped AbortSignal releases their disposer.
        const off = this.subscribeEvent(owner, type, listener);
        controller.signal.addEventListener("abort", off, { once: true });
        return off;
      },
    };
  }
  async execute(key: string, context = this.context) {
    if (!this.canExecute(key, context)) {
      throw new Error("Command is unavailable.");
    }
    const [owner, id] = key.split("/");
    const api = this.api(owner, context);
    try {
      const handler = this.extensions.get(owner)?.handlers?.[id];
      if (handler === undefined) {
        throw new Error("Command is unavailable.");
      }
      return await handler(api);
    } finally {
      api.dispose();
    }
  }
  run(key: string, context = this.context) {
    startAction(this.execute(key, context), this.host.error.bind(this.host));
  }
  // Recurring contribution events have synchronous subscribe/dispose semantics.
  private subscribeEvent(
    owner: string,
    type: ContributionEvent["type"],
    listener: (event: ContributionEvent) => void,
  ) {
    const event = { owner, type, listener };
    this.events.add(event);
    return () => {
      this.events.delete(event);
    };
  }
  emit(event: ContributionEvent) {
    // Snapshot dispatch excludes registrations made by listeners during this publication.
    // oxlint-disable-next-line unicorn/no-useless-spread
    for (const listener of [...this.events]) {
      if (listener.type === event.type) {
        try {
          listener.listener(event);
        } catch (error) {
          this.host.error(error);
        }
      }
    }
  }
}

function supports(clients: UiClient[] | undefined, context: ContributionContext) {
  return clients === undefined || clients.length === 0 || clients.includes(context.client);
}

function validateCommands(extension: ClientExtension) {
  const commands = new Set((extension.manifest.commands ?? []).map((command) => command.id));
  for (const command of extension.manifest.commands ?? []) {
    if (
      !extension.handlers ||
      !Object.hasOwn(extension.handlers, command.id) ||
      typeof extension.handlers[command.id] !== "function"
    ) {
      throw new Error(`Missing command handler: ${command.id}`);
    }
    if (hasText(command.shortcut) && !/^(?:Mod\+)(?:Shift\+)?[a-z0-9]$/iu.test(command.shortcut)) {
      throw new Error("Shortcuts use Mod+[Shift+]key.");
    }
  }
  return commands;
}
