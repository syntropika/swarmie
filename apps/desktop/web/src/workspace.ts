import type {
  AgentTask,
  PromptContent,
  PreparedInput,
  NativeClientMethods,
  ProjectRecord,
  RuntimeIdentity,
  LocalStoreIdentity,
  SubmissionProjection,
  SessionOpenProjection,
  SessionControls,
} from "@swarmie/contracts";
import { decodeContract } from "@swarmie/contracts/validation";

import { startAction } from "~/web/src/action-boundary";
import { WorkspaceCache, hydrateThreads } from "~/web/src/cache";
import { clientRequest, nativeClient } from "~/web/src/client-transport";
import { sortedCopy } from "~/web/src/collection-order";
import {
  RpcError,
  isThreadKey,
  runtimeUnavailable,
  staleConnectionResponse,
  isRuntimeUnavailable,
  isStaleConnectionResponse,
  decodeClientResult,
  decodeCachedThreads,
  decodeCachedProjects,
  decodePathsResult,
  decodeFileResult,
  decodeProjectOpenResult,
  decodeReviewResult,
} from "~/web/src/connections";
import type { ConnectionMethod, ConnectionMethodResults } from "~/web/src/connections";
import { contextText, lineExcerpt } from "~/web/src/context";
import type { ContextItem, FileLocation } from "~/web/src/context";
import { defaultHarness, readHarnessSetup, setHarnessEnabled } from "~/web/src/harness-setup";
import {
  retainNode,
  sameNode,
  validNodeIdentity,
  workspaceStorageKey,
  rpcPath,
  routeTargetKey,
} from "~/web/src/node-scope";
import { NotificationService, readNotificationPreferences } from "~/web/src/notifications";
import type { NotificationPreferences } from "~/web/src/notifications";
import { newThread, reduceEvents, isRecord, nativeEvents } from "~/web/src/protocol";
import type {
  Thread,
  Project,
  Event,
  Detection,
  Review,
  SessionSnapshot,
  ExtensionInfo,
  SubmissionReceipt,
  Attachment,
  PromptPart,
  Queued,
} from "~/web/src/protocol";
import type { AgentToolConnect, AgentToolSetup } from "~/web/src/provider-setup";
import { fingerprint } from "~/web/src/reviewed";
import { hasText, nonEmptyText, hasNumber } from "~/web/src/value-presence";

export { RpcError } from "~/web/src/connections";

export type NodeWorkspaceOptions = {
  node?: LocalStoreIdentity;
  rpcPath?: string;
};
export type Workspace = {
  node?: LocalStoreIdentity;
  projects: Project[];
  threads: Thread[];
  activeId: string;
  harnesses: Detection[];
  disabledHarnesses: string[];
  connected: boolean;
  git: boolean;
  busy: boolean;
  error: string;
  review?: Review;
  extensions: ExtensionInfo[];
  cacheSaving: boolean;
  notifications: NotificationPreferences;
  chatDirectory: string;
  customChatDirectory?: string;
};
type Listener = () => void;
function validIdentity(value: unknown): value is RuntimeIdentity {
  return (
    isRecord(value) &&
    typeof value.incarnationId === "string" &&
    value.incarnationId.length > 0 &&
    validNodeIdentity(value.identity)
  );
}
function mergeConfiguration(defaults: Record<string, unknown>, overrides: Record<string, unknown>) {
  const result = {
    ...defaults,
  };
  for (const [key, value] of Object.entries(overrides)) {
    const base = defaults[key];
    const merged = isRecord(value) && isRecord(base) ? mergeConfiguration(base, value) : value;
    Object.defineProperty(result, key, {
      value: merged,
      enumerable: true,
      configurable: true,
      writable: true,
    });
  }
  return result;
}
async function promptKey(content: readonly PromptContent[]) {
  return await fingerprint(
    JSON.stringify(
      content.map((part) => {
        if (part.type === "image") {
          return [part.type, part.data.mime_type, part.data.data];
        }
        if (part.type === "skill") {
          return [part.type, part.data.name, part.data.path];
        }
        return [part.type, part.data];
      }),
    ),
  );
}
export class WorkspaceStore {
  private readonly listeners = new Set<Listener>();
  private polling = false;
  private requestId = 0;
  private token = "";
  private runtimeIdentity?: RuntimeIdentity;
  private retainedTarget?: LocalStoreIdentity;
  private readonly endpoint: string;
  private readonly targetStorageKey: string;
  private connectionEpoch = 0;
  private connection?: Promise<void>;
  private connecting = false;
  private reconnectAt = Infinity;
  private reconnectDelay = 1000;
  private heartbeatAt = 0;
  private readonly notificationService = new NotificationService();
  private cache?: WorkspaceCache;
  private readonly edited = new Map<string, Set<keyof Thread>>();
  private readonly configurationEdited = new Set<string>();
  private readonly defaultsPending = new Map<string, Promise<void>>();
  private readonly launching = new Map<string, Promise<Thread>>();
  private readonly controlPreviews = new Map<string, Promise<void>>();
  private readonly messageTimes = new Map<string, Promise<void>>();
  private hydration: Promise<void>;
  private cachePending = false;
  private cacheWriter?: Promise<void>;
  private revision = 0;
  private saveTimer?: ReturnType<typeof setTimeout>;
  private snapshot: Workspace = {
    projects: [],
    threads: [],
    activeId: "",
    harnesses: [],
    disabledHarnesses: [],
    connected: false,
    git: false,
    busy: false,
    error: "",
    extensions: [],
    cacheSaving: false,
    notifications: readNotificationPreferences(),
    chatDirectory: "",
  };
  getSnapshot = () => this.snapshot;
  subscribe = (listener: Listener): (() => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };
  constructor(options: NodeWorkspaceOptions = {}) {
    this.endpoint = rpcPath(options.rpcPath);
    this.targetStorageKey = routeTargetKey(this.endpoint);
    let { node } = options;
    if (!node) {
      try {
        const saved: unknown = JSON.parse(localStorage.getItem(this.targetStorageKey) ?? "null");
        if (validNodeIdentity(saved)) {
          node = saved;
        }
      } catch {
        /* Corrupt route metadata must not prevent startup. */
      }
    }
    if (node) {
      this.retainedTarget = retainNode(node);
      this.snapshot.node = this.retainedTarget;
    }
    this.hydration = this.retainedTarget
      ? this.loadWorkspace(this.retainedTarget)
      : Promise.resolve();
    if (typeof window !== "undefined") {
      window.addEventListener("pagehide", () => {
        this.persist();
      });
    }
  }
  private async loadWorkspace(node: LocalStoreIdentity) {
    this.snapshot = {
      ...this.snapshot,
      disabledHarnesses: readHarnessSetup(node).disabledHarnesses,
    };
    this.cache = new WorkspaceCache(node);
    try {
      const saved: unknown = JSON.parse(localStorage.getItem(workspaceStorageKey(node)) ?? "null");
      if (
        isRecord(saved) &&
        saved.version === 3 &&
        validNodeIdentity(saved.node) &&
        sameNode(saved.node, node) &&
        Array.isArray(saved.projects) &&
        Array.isArray(saved.threads)
      ) {
        this.snapshot = {
          ...this.snapshot,
          projects: decodeCachedProjects(saved.projects),
          threads: decodeCachedThreads(saved.threads),
          activeId: typeof saved.activeId === "string" ? saved.activeId : "",
          customChatDirectory:
            typeof saved.customChatDirectory === "string" ? saved.customChatDirectory : undefined,
        };
      }
    } catch {
      /* Corrupt node metadata must not prevent startup. */
    }
    try {
      const saved = await this.cache.load();
      if (saved) {
        const current = this.snapshot.threads;
        const nativeThreads = saved.threads.map(
          (thread) => current.find((item) => item.id === thread.id) ?? thread,
        );
        const ids = new Set(nativeThreads.map((thread) => thread.id));
        nativeThreads.push(...current.filter((thread) => !ids.has(thread.id)));
        this.snapshot = {
          ...this.snapshot,
          ...(nativeClient() && saved.metadata
            ? {
                ...saved.metadata,
                activeId: this.snapshot.activeId || saved.metadata.activeId,
              }
            : {}),
          threads: hydrateThreads(
            nativeClient() ? nativeThreads : current,
            saved.threads,
            this.edited,
          ),
        };
      }
      for (const listener of this.listeners) {
        listener();
      }
    } catch (error) {
      this.storageError(error);
    }
  }
  private publish(patch: Partial<Workspace>) {
    this.snapshot = {
      ...this.snapshot,
      ...patch,
    };
    for (const listener of this.listeners) {
      listener();
    }
    clearTimeout(this.saveTimer);
    this.saveTimer = setTimeout(() => {
      this.persist();
    }, 500);
  }
  private storageError(error: unknown) {
    this.snapshot = {
      ...this.snapshot,
      error: `Draft storage failed: ${error instanceof Error ? error.message : String(error)}. Keep this window open and free disk space before closing.`,
    };
    for (const listener of this.listeners) {
      listener();
    }
  }
  private persist(requireIdentityStorage = false) {
    if (!this.retainedTarget) {
      if (requireIdentityStorage) {
        throw new Error("Verify a node before recording submission intent.");
      }
      return;
    }
    this.revision += 1;
    try {
      const { projects, threads, activeId, customChatDirectory } = this.snapshot;
      localStorage.setItem(
        workspaceStorageKey(this.retainedTarget),
        JSON.stringify({
          version: 3,
          node: this.retainedTarget,
          projects,
          activeId,
          customChatDirectory,
          revision: this.revision,
          threads: threads.map((thread) => ({
            ...thread,
            attachments: undefined,
            fileDrafts: undefined,
            promptRecords: undefined,
            messages: [],
            tools: [],
            permissions: [],
            reasoning: undefined,
            plan: undefined,
            subagents: undefined,
            usage: undefined,
            context: undefined,
            queue: undefined,
          })),
        }),
      );
    } catch (error) {
      this.storageError(error);
      if (requireIdentityStorage) {
        throw error;
      }
    }
    if (!(this.cache?.available() === true)) {
      if (
        this.snapshot.threads.some(
          (thread) =>
            hasNumber(thread.attachments?.length) ||
            Object.keys(thread.fileDrafts ?? {}).length > 0 ||
            hasNumber(thread.promptRecords?.length),
        )
      ) {
        this.storageError("Persistent attachment storage is unavailable");
      }
      return;
    }
    this.cachePending = true;
    if (this.cacheWriter) {
      return;
    }
    this.snapshot = {
      ...this.snapshot,
      cacheSaving: true,
    };
    for (const listener of this.listeners) {
      listener();
    }
    this.cacheWriter = this.writeCache();
  }
  private async writeCache() {
    await this.hydration;
    try {
      while (this.cachePending) {
        this.cachePending = false;
        // oxlint-disable-next-line eslint/no-await-in-loop -- Cache revisions are written serially; an older write must never overwrite a newer revision.
        await this.requireCache().save({
          threads: this.snapshot.threads,
          revision: this.revision,
          metadata: {
            projects: this.snapshot.projects,
            activeId: this.snapshot.activeId,
            customChatDirectory: this.snapshot.customChatDirectory,
          },
        });
      }
    } catch (error) {
      this.storageError(error);
    } finally {
      this.cacheWriter = undefined;
      this.snapshot = {
        ...this.snapshot,
        cacheSaving: false,
      };
      for (const listener of this.listeners) {
        listener();
      }
    }
  }
  async flushPersistence() {
    await this.hydration;
    this.persist();
    await this.cacheWriter;
  }
  async ready() {
    await this.hydration;
  }
  updateThread(id: string, patch: Partial<Thread>, touch = true) {
    const fields = this.edited.get(id) ?? new Set<keyof Thread>();
    for (const key of Object.keys(patch)) {
      if (isThreadKey(key)) {
        fields.add(key);
      }
    }
    this.edited.set(id, fields);
    if (["harness", "model", "worktreeId"].some((key) => key in patch)) {
      this.configurationEdited.add(id);
    }
    this.publish({
      threads: this.snapshot.threads.map((t) =>
        t.id === id
          ? {
              ...t,
              ...patch,
              ...(touch
                ? {
                    updatedAtMs: Date.now(),
                  }
                : {}),
            }
          : t,
      ),
    });
    if ("draft" in patch || "attachments" in patch) {
      this.persist();
    }
  }
  private requireThread(id: string): Thread {
    const thread = this.snapshot.threads.find((item) => item.id === id);
    if (!thread) {
      throw new Error("The conversation is no longer available.");
    }
    return thread;
  }
  private requireCache(): WorkspaceCache {
    if (!this.cache) {
      throw new Error("Persistent draft storage is unavailable.");
    }
    return this.cache;
  }
  private requireNode(): LocalStoreIdentity {
    if (!this.retainedTarget) {
      throw new Error("Connect to a verified node before using it.");
    }
    return this.retainedTarget;
  }
  active() {
    return this.snapshot.threads.find((t) => t.id === this.snapshot.activeId);
  }
  async findFiles(query: string) {
    const thread = this.active();
    if (!hasText(thread?.worktreeId)) {
      return [];
    }
    const result = await this.invokeExtension(
      "workspace",
      "files.find",
      {
        projectId: thread.worktreeId,
        query,
        limit: 80,
      },
      decodePathsResult,
    );
    return result.paths;
  }
  openFile(location: FileLocation) {
    const thread = this.active();
    if (!thread) {
      return;
    }
    const relative = location.path.startsWith(`${thread.worktreeId}/`)
      ? location.path.slice(thread.worktreeId.length + 1)
      : location.path;
    const file = {
      ...location,
      path: relative,
    };
    const files = [...(thread.layout?.files ?? []).filter((f) => f.path !== relative), file];
    if (files.length > 8) {
      throw new Error("Keep up to eight files open in a conversation. Close a file tab first.");
    }
    this.updateThread(thread.id, {
      layout: {
        ...thread.layout,
        files,
        activeFile: file,
        panel: "extensions",
        selectedView: "workspace:files",
      },
    });
    window.dispatchEvent(new CustomEvent("swarmie:open-tools"));
  }
  addContext(id: string, item: Omit<ContextItem, "id">) {
    const thread = this.snapshot.threads.find((t) => t.id === id);
    if (!thread) {
      return;
    }
    const items = [
      ...(thread.contextItems ?? []),
      {
        ...item,
        id: crypto.randomUUID(),
      },
    ];
    if (new TextEncoder().encode(contextText(items)).length > 1024 * 1024) {
      throw new Error("Keep selected context under 1 MB.");
    }
    this.updateThread(id, {
      contextItems: items,
    });
  }
  async addFileContext(id: string, location: FileLocation) {
    const thread = this.snapshot.threads.find((t) => t.id === id);
    if (!thread) {
      return;
    }
    const result = await this.invokeExtension(
      "workspace",
      "files.read",
      {
        projectId: thread.worktreeId,
        path: location.path,
        sessionId: thread.sessionId,
      },
      decodeFileResult,
    );
    const text = lineExcerpt(result.content, location.startLine, location.endLine);
    this.addContext(id, {
      kind: "file",
      label: `${location.path}${hasNumber(location.startLine) ? `:${location.startLine}-${location.endLine ?? location.startLine}` : ""}`,
      text,
      ...location,
    });
  }
  reorderThread(id: string, before: string) {
    const threads = [...this.snapshot.threads];
    const index = threads.findIndex((t) => t.id === id);
    if (index === -1 || id === before) {
      return;
    }
    const [thread] = threads.splice(index, 1);
    const target = threads.findIndex((t) => t.id === before);
    threads.splice(target === -1 ? threads.length : target, 0, thread);
    this.publish({
      threads,
    });
  }
  orderThreads(ids: string[]) {
    const order = new Map(ids.map((id, index) => [id, index]));
    this.publish({
      threads: sortedCopy(
        this.snapshot.threads,
        (a, b) => (order.get(a.id) ?? ids.length) - (order.get(b.id) ?? ids.length),
      ),
    });
  }
  select(id: string) {
    this.publish({
      activeId: id,
      review: undefined,
      error: "",
    });
  }
  setError(error: unknown) {
    if (isStaleConnectionResponse(error)) {
      return;
    }
    this.publish({
      error: error instanceof Error ? error.message : String(error),
      busy: false,
    });
  }
  dismissError() {
    this.publish({
      error: "",
    });
  }
  private unavailable(error: unknown) {
    this.reconnectAt = Date.now() + this.reconnectDelay;
    this.reconnectDelay = Math.min(30_000, this.reconnectDelay * 2);
    this.publish({
      connected: false,
      busy: false,
      error: "Runtime is unavailable. Your draft is saved. Reconnecting…",
    });
    return runtimeUnavailable(error instanceof Error ? error.message : String(error));
  }
  async rpc<M extends ConnectionMethod>(
    method: M,
    params: unknown = {},
  ): Promise<ConnectionMethodResults[M]> {
    return decodeClientResult(method, await this.request(method, params));
  }
  async request(method: ConnectionMethod, params: unknown = {}): Promise<unknown> {
    return await this.rpcAt(this.endpoint, method, params);
  }
  previewPath(path: string): string {
    if (!/^\/preview\/[0-9a-f-]+\//iu.test(path)) {
      throw new Error("Invalid node preview path.");
    }
    return this.endpoint === "/rpc" ? path : this.endpoint.slice(0, -4) + path;
  }
  disconnect() {
    this.connection = undefined;
    this.connectionEpoch += 1;
    this.runtimeIdentity = undefined;
    this.reconnectAt = Infinity;
    this.publish({
      connected: false,
      busy: false,
      error: "",
    });
  }
  observeDisconnected() {
    this.connectionEpoch += 1;
    this.runtimeIdentity = undefined;
    this.reconnectAt = Infinity;
    this.publish({
      connected: false,
      error: "The node connection was interrupted. Reconnecting…",
    });
  }
  private async rpcAt(
    endpoint: string,
    method: ConnectionMethod,
    params: unknown,
  ): Promise<unknown> {
    if (method !== "runtime/initialize" && !this.runtimeIdentity) {
      throw new Error("Connect to the runtime before using it.");
    }
    if (method !== "runtime/initialize" && !this.snapshot.connected && !this.connecting) {
      throw runtimeUnavailable("Reconnect to the runtime before continuing.");
    }
    const epoch = this.connectionEpoch;
    const target = this.runtimeIdentity;
    const assertCurrent = () => {
      if (epoch !== this.connectionEpoch || target !== this.runtimeIdentity) {
        throw staleConnectionResponse(
          "This response belongs to an earlier runtime connection. Reconnect to read authoritative state; the original command may have completed.",
        );
      }
    };
    let response: Response;
    try {
      response = await clientRequest(
        endpoint,
        {
          jsonrpc: "2.0",
          id: (this.requestId += 1),
          method,
          params,
          ...(target
            ? {
                expectedRuntime: target,
              }
            : {}),
        },
        this.token,
        method === "runtime/initialize" ? 5000 : 120_000,
      );
    } catch (error) {
      assertCurrent();
      throw this.unavailable(error);
    }
    assertCurrent();
    if (!response.ok) {
      this.reconnectAt = Infinity;
      this.publish({
        connected: false,
      });
      throw new Error(
        response.status === 401
          ? "Enter the runtime token in Settings, then reconnect."
          : `The runtime returned HTTP ${response.status}. Reconnect in Settings.`,
      );
    }
    const body: unknown = await response.json();
    assertCurrent();
    if (!isRecord(body)) {
      throw new TypeError("Invalid RPC envelope.");
    }
    if (body.error !== undefined && body.error !== null) {
      if (
        !isRecord(body.error) ||
        typeof body.error.code !== "number" ||
        typeof body.error.message !== "string"
      ) {
        throw new TypeError("Invalid RPC error envelope.");
      }
      if (body.error.code === -32_006) {
        this.unavailable(new Error(body.error.message));
      }
      if (body.error.code === -32_005) {
        this.reconnectAt = Date.now() + 1000;
        this.publish({
          connected: false,
          error: "The runtime changed. Reconnecting before continuing…",
        });
      }
      throw new RpcError(body.error.message, body.error.code, body.error.data);
    }
    return body.result;
  }
  extension(command: string, args: unknown): Promise<unknown>;
  extension<T>(command: string, args: unknown, decode: (value: unknown) => T): Promise<T>;
  async extension(
    command: string,
    args: unknown,
    decode?: (value: unknown) => unknown,
  ): Promise<unknown> {
    const value = await this.invokeExtension("git", command, args);
    return decode ? decode(value) : value;
  }
  async setNotifications(preferences: NotificationPreferences) {
    await this.notificationService.configure(preferences);
    this.publish({
      notifications: preferences,
    });
  }
  async probe(harness: string) {
    const result = await this.rpc("runtime/probe", {
      harness,
    });
    this.publish({
      harnesses: this.snapshot.harnesses.map((h) =>
        h.harness === harness
          ? {
              ...h,
              ...result,
            }
          : h,
      ),
    });
    return result;
  }
  async agentToolsSetup(harness: string) {
    const setup = await this.rpc("runtime/agentToolsSetup", {
      harness,
    });
    this.mergeAgentToolsSetup(setup);
    return setup;
  }
  private mergeAgentToolsSetup(setup: AgentToolSetup) {
    if (!(setup.manual === true) && Array.isArray(setup.providers)) {
      this.publish({
        harnesses: this.snapshot.harnesses.map((item) =>
          item.harness === setup.harness
            ? {
                ...item,
                available: setup.installed,
                authState: setup.authState,
                message: setup.message,
              }
            : item,
        ),
      });
    }
  }
  async agentToolsConnect(input: AgentToolConnect) {
    return await this.rpc("runtime/agentToolsConnect", input);
  }
  async agentToolsFlow(id: string) {
    return await this.rpc("runtime/agentToolsFlow", {
      id,
    });
  }
  async agentToolsInstall(harness: string) {
    return await this.rpc("runtime/agentToolsInstall", {
      harness,
    });
  }
  async agentToolsInput(id: string, values: Record<string, string>) {
    return await this.rpc("runtime/agentToolsInput", {
      id,
      values,
    });
  }
  async agentToolsCancel(id: string) {
    return await this.rpc("runtime/agentToolsCancel", {
      id,
    });
  }
  async agentToolsDisconnect(harness: string) {
    const setup = await this.rpc("runtime/agentToolsDisconnect", {
      harness,
    });
    this.mergeAgentToolsSetup(setup);
    return setup;
  }
  setHarnessEnabled(harness: string, enabled: boolean) {
    if (!this.snapshot.node) {
      throw new Error("Connect to a computer before changing its agent tools.");
    }
    if (
      enabled &&
      !this.snapshot.harnesses.some((item) => item.harness === harness && item.available)
    ) {
      throw new Error("Install this agent tool before enabling it.");
    }
    this.publish({
      disabledHarnesses: setHarnessEnabled(this.snapshot.node, harness, enabled),
    });
  }
  async connect(token = this.token): Promise<void> {
    if (this.connection) {
      await this.connection;
      return;
    }
    const connection = this.connectRuntime(token).finally(() => {
      if (this.connection === connection) {
        this.connection = undefined;
      }
    });
    this.connection = connection;
    await connection;
  }
  private async connectRuntime(token: string) {
    this.connectionEpoch += 1;
    const { connectionEpoch: epoch } = this;
    await this.hydration;
    if (epoch !== this.connectionEpoch) {
      return;
    }
    this.token = token;
    this.publish({
      busy: true,
      error: "",
      connected: false,
    });
    this.connecting = true;
    this.runtimeIdentity = undefined;
    this.reconnectAt = Infinity;
    try {
      const initialized = await this.rpc("runtime/initialize");
      if (
        initialized.name !== "swarmie" ||
        initialized.protocolVersion !== "1" ||
        !validIdentity(initialized.runtimeIdentity)
      ) {
        throw new Error("This runtime does not support the required connection protocol.");
      }
      const target = initialized.runtimeIdentity;
      if (this.retainedTarget && !sameNode(this.retainedTarget, target.identity)) {
        throw new Error(
          "This connection now points to a different environment. Your saved chats and drafts have been kept.",
        );
      }
      const node = this.retainedTarget ?? retainNode(target.identity);
      localStorage.setItem(this.targetStorageKey, JSON.stringify(node));
      if (!this.retainedTarget) {
        const drafts = this.snapshot.threads.filter((thread) => !hasText(thread.sessionId));
        const { activeId } = this.snapshot;
        this.retainedTarget = node;
        this.snapshot = {
          ...this.snapshot,
          node,
        };
        this.hydration = this.loadWorkspace(node);
        await this.hydration;
        if (epoch !== this.connectionEpoch) {
          return;
        }
        const ids = new Set(this.snapshot.threads.map((thread) => thread.id));
        const fresh = drafts.filter((thread) => !ids.has(thread.id));
        if (fresh.length) {
          this.publish({
            threads: [...fresh, ...this.snapshot.threads],
            activeId,
          });
        }
      }
      this.runtimeIdentity = Object.freeze({
        identity: node,
        incarnationId: target.incarnationId,
      });
      const [harnesses, extensions] = await Promise.all([
        this.rpc("runtime/discover"),
        this.rpc("extension/list"),
      ]);
      this.publish({
        harnesses,
        git: extensions.some((e) => e.id === "git"),
        extensions,
        connected: true,
        busy: false,
      });
      if (extensions.some((e) => e.id === "workspace")) {
        try {
          const path = await this.resolveChatDirectory();
          for (const thread of this.snapshot.threads.filter((t) => !t.projectId && !t.worktreeId)) {
            this.updateThread(
              thread.id,
              {
                worktreeId: path,
              },
              false,
            );
          }
        } catch (error) {
          this.setError(error);
        }
      }
      this.publish({
        projects: await this.rpc("client/projectCatalog"),
      });
      await this.restoreSessions();
      if (!this.snapshot.threads.length) {
        this.createThread();
      }
      await this.restoreRetainedThreads();
      this.reconnectDelay = 1000;
      this.heartbeatAt = Date.now() + 5000;
    } catch (error) {
      if (epoch !== this.connectionEpoch) {
        return;
      }
      this.publish({
        connected: false,
      });
      if (!isRuntimeUnavailable(error)) {
        this.setError(error);
      }
    } finally {
      if (epoch === this.connectionEpoch) {
        this.connecting = false;
      }
    }
  }
  async openProjectRecord(path: string, retained?: ProjectRecord): Promise<Project> {
    return await this.rpc(
      retained ? "client/projectGet" : "client/projectOpen",
      retained
        ? {
            projectId: retained.id,
          }
        : {
            path,
          },
    );
  }
  async openWorkspace(thread: Thread) {
    const project = this.snapshot.projects.find((p) => p.id === thread.projectId);
    const cwd =
      project?.worktrees.find((w) => w.id === thread.worktreeId)?.path ?? thread.worktreeId;
    if (!cwd) {
      throw new Error("Open a project first.");
    }
    return await this.invokeExtension(
      "workspace",
      "project.open",
      {
        path: cwd,
        sessionId: thread.sessionId,
      },
      decodeProjectOpenResult,
    );
  }
  private async restoreRetainedThreads() {
    for (const thread of this.snapshot.threads.filter((t) => hasText(t.sessionId))) {
      try {
        // oxlint-disable-next-line eslint/no-await-in-loop -- Restore must stop immediately if this connection loses ownership before touching another retained chat.
        await this.restoreThread(thread.id);
      } catch (error) {
        if (!this.snapshot.connected) {
          throw error;
        }
        this.updateThread(
          thread.id,
          {
            state: "closed",
            permissions: [],
            error: String(error),
          },
          false,
        );
      }
    }
  }
  async openProject(path: string, name?: string) {
    this.publish({
      busy: true,
      error: "",
    });
    try {
      const opened = await this.rpc("client/projectOpen", {
        path,
        ...(hasText(name?.trim())
          ? {
              name: name.trim(),
            }
          : {}),
      });
      const project = {
        ...opened,
        name:
          nonEmptyText(name?.trim()) ??
          nonEmptyText(this.snapshot.projects.find((p) => p.id === opened.id)?.name) ??
          nonEmptyText(opened.name) ??
          nonEmptyText(opened.path.replace(/\/+$/u, "").split("/").at(-1)) ??
          opened.path,
      };
      this.publish({
        projects: [...this.snapshot.projects.filter((p) => p.id !== project.id), project],
        busy: false,
      });
      this.createThread(project.id);
      return project;
    } catch (error) {
      this.setError(error);
      return undefined;
    }
  }
  async createProject(
    source: "create" | "clone" | "scratch",
    args: Record<string, unknown> = {},
    name?: string,
  ) {
    const project = await this.rpc("client/projectCreate", {
      source,
      arguments: args,
      ...(hasText(name)
        ? {
            name,
          }
        : {}),
    });
    this.publish({
      projects: [...this.snapshot.projects.filter((p) => p.id !== project.id), project],
    });
    this.createThread(project.id);
    return project;
  }
  async sessionIntent(id: string) {
    const pending = this.defaultsPending.get(id);
    if (pending) {
      await pending;
    }
    return launchIntent(this.requireThread(id));
  }
  async saveSchedule(id: string, schedule: Record<string, unknown>) {
    const thread = this.requireThread(id);
    const applyCreated = (session: SessionSnapshot) => {
      this.updateThread(id, {
        sessionId: session.id,
        worktreeId: session.options.cwd,
        state: session.state,
        capabilities: session.capabilities,
        executionPolicy: session.executionPolicy,
      });
    };
    try {
      const result = await this.rpc("client/schedulePut", {
        schedule: {
          ...schedule,
          sessionId: schedule.sessionId ?? thread.sessionId,
        },
        launch: await this.sessionIntent(id),
      });
      if (result.created) {
        applyCreated(result.session);
      }
      return result;
    } catch (error) {
      const created =
        error instanceof RpcError && isRecord(error.data) && error.data.createdSession !== undefined
          ? decodeContract("SessionSnapshot", error.data.createdSession)
          : undefined;
      if (created) {
        applyCreated(created);
      }
      throw error;
    }
  }
  async importSession(query: unknown) {
    const result = await this.rpc("client/sessionImport", query);
    let thread = this.snapshot.threads.find((t) => t.sessionId === result.snapshot.id);
    if (!thread) {
      const { snapshot } = result;
      const source = snapshot.workingContext?.source;
      thread = {
        ...newThread(
          source?.kind === "project_folder" ? source.projectId : "",
          snapshot.options.cwd,
          snapshot.options.harness,
        ),
        sessionId: snapshot.id,
        title: snapshot.metadata?.title ?? "Recovered chat",
        model: snapshot.options.model ?? "",
        config: isRecord(snapshot.options.config) ? snapshot.options.config : {},
      };
      this.publish({
        threads: [thread, ...this.snapshot.threads],
      });
    }
    await this.applyProjection(thread.id, result);
    this.select(thread.id);
    return thread.id;
  }
  private async resolveChatDirectory() {
    const directory = await this.rpc("client/chatDirectory");
    this.publish({
      chatDirectory: directory.path,
    });
    return directory.path;
  }
  async setChatDirectory(path?: string) {
    const directory = await this.rpc("client/chatDirectory", {
      set: true,
      ...(hasText(path?.trim())
        ? {
            path: path.trim(),
          }
        : {}),
    });
    this.publish({
      chatDirectory: directory.path,
      customChatDirectory: hasText(path?.trim()) ? directory.path : undefined,
    });
    this.persist();
  }
  createThread(projectId?: string) {
    const project = this.snapshot.projects.find((p) => p.id === projectId);
    const thread = newThread(
      project?.id ?? "",
      project?.worktrees[0]?.id ?? this.snapshot.chatDirectory,
      defaultHarness(this.snapshot.harnesses, this.snapshot.node) ?? "codex",
    );
    if (project) {
      thread.projectDirectory = project.path;
      thread.projectRevision = project.revision;
    }
    this.publish({
      threads: [thread, ...this.snapshot.threads],
      activeId: thread.id,
      review: undefined,
      error: "",
    });
    if (!project && !thread.worktreeId && this.snapshot.connected) {
      const pending = (async () => {
        const path = await this.resolveChatDirectory();
        this.updateThread(thread.id, { worktreeId: path }, false);
      })();
      this.defaultsPending.set(thread.id, pending);
      startAction(pending, this.setError.bind(this));
    }
    if (
      project &&
      thread.worktreeId &&
      (this.snapshot.connected || this.snapshot.extensions.some((e) => e.id === "resources"))
    ) {
      const pending = (async () => {
        const defaults = await this.rpc("client/projectDefaults", { projectId: project.id });
        const edited = this.configurationEdited.has(thread.id);
        if (
          hasText(defaults.cwd) &&
          !project?.worktrees.some(
            (worktree) => worktree.path === defaults.cwd || worktree.id === defaults.cwd,
          )
        ) {
          throw new Error(
            "The saved project workspace is unavailable. Choose an available workspace in project defaults.",
          );
        }
        const current = this.requireThread(thread.id);
        this.updateThread(thread.id, {
          harness: edited ? current.harness : (defaults.harness ?? thread.harness),
          model: edited ? current.model : (defaults.model ?? thread.model),
          config: edited
            ? current.config
            : mergeConfiguration(
                isRecord(defaults.config) ? defaults.config : {},
                current.config ?? {},
              ),
          worktreeId:
            current.worktreeId === thread.worktreeId
              ? (defaults.cwd ?? current.worktreeId)
              : current.worktreeId,
          projectDirectory: defaults.cwd ?? undefined,
          projectRevision: defaults.revision,
        });
      })();
      this.defaultsPending.set(thread.id, pending);
      startAction(pending, this.setError.bind(this));
    }
  }
  async restoreSessions() {
    const sessions = await this.rpc("session/list");
    for (const session of sessions) {
      if (this.snapshot.threads.some((t) => t.sessionId === session.id)) {
        continue;
      }
      const source = session.workingContext?.source;
      const project =
        source?.kind === "project_folder"
          ? this.snapshot.projects.find((p) => p.id === source.projectId)
          : undefined;
      const thread = newThread(project?.id ?? "", session.options.cwd, session.options.harness);
      this.publish({
        threads: [
          ...this.snapshot.threads,
          {
            ...thread,
            sessionId: session.id,
            title: session.metadata?.title ?? "Recovered chat",
            archived: session.metadata?.archived,
            model: session.options.model ?? "",
            config: isRecord(session.options.config) ? session.options.config : {},
            createdAtMs: session.metadata?.createdAtMs || thread.createdAtMs,
            updatedAtMs: session.metadata?.updatedAtMs || thread.updatedAtMs,
            state: session.state,
            capabilities: session.capabilities,
            executionPolicy: session.executionPolicy,
          },
        ],
      });
    }
  }
  async loadMessageTimes(id: string): Promise<void> {
    const thread = this.snapshot.threads.find((item) => item.id === id);
    if (!hasText(thread?.sessionId) || !this.snapshot.connected) {
      return;
    }
    const { sessionId } = thread;
    const key = `${id}:${sessionId}`;
    const pending = this.messageTimes.get(key);
    if (pending) {
      await pending;
      return;
    }
    const operation = (async () => {
      const times = new Map<string, Date>();
      const projection = await this.rpc("client/sessionProjection", {
        sessionId,
      });
      for (const event of projection.events) {
        if (
          event.type === "message.user" &&
          typeof event.timestampMs === "number" &&
          Number.isFinite(event.timestampMs) &&
          event.timestampMs > 0 &&
          event.timestampMs <= 8_640_000_000_000_000
        ) {
          times.set(`user-${event.sequence}`, new Date(event.timestampMs));
        }
      }
      const latest = this.snapshot.threads.find((item) => item.id === id);
      if (!latest || latest.sessionId !== sessionId) {
        return;
      }
      let changed = false;
      const messages = latest.messages.map((message) => {
        if (message.role !== "user") {
          return message;
        }
        const date = message.createdAt ? new Date(message.createdAt) : undefined;
        const saved = hasText(message.id) ? times.get(message.id) : undefined;
        if (saved && (!date || !Number.isFinite(date.getTime()))) {
          changed = true;
          return {
            ...message,
            createdAt: saved,
          };
        }
        return message;
      });
      if (changed) {
        this.updateThread(
          id,
          {
            messages,
          },
          false,
        );
      }
    })().finally(() => {
      this.messageTimes.delete(key);
    });
    this.messageTimes.set(key, operation);
    await operation;
  }
  async restoreThread(id: string) {
    const thread = this.snapshot.threads.find((t) => t.id === id);
    if (!hasText(thread?.sessionId)) {
      return;
    }
    const projection = await this.rpc("client/sessionOpen", {
      sessionId: thread.sessionId,
    });
    await this.applyProjection(id, projection);
  }
  private async applyProjection(id: string, projection: SessionOpenProjection) {
    const thread = this.snapshot.threads.find((t) => t.id === id);
    if (!thread || thread.sessionId !== projection.snapshot.id) {
      return;
    }
    await this.recoverSubmissions(id, false, projection);
    const { snapshot } = projection;
    let replay = replayThread(
      thread,
      this.snapshot.threads.find((t) => t.id === id)?.promptRecords,
    );
    replay = reduceEvents(
      await this.bindPromptRecords(replay, nativeEvents(projection.events)),
      nativeEvents(projection.events),
    );
    const current = snapshot;
    let { state } = snapshot;
    if (projection.unresolved) {
      state = "needs_input";
    } else if (projection.activation?.status === "unavailable") {
      state = "closed";
    }
    const archived = snapshot.metadata?.archived ?? thread.archived;
    const latest = this.requireThread(id);
    const eventFields: (keyof Thread)[] = [
      "messages",
      "tools",
      "tasks",
      "permissions",
      "sequence",
      "reasoning",
      "plan",
      "subagents",
      "usage",
      "context",
      "queue",
      "queuePaused",
      "turnGroups",
      "assistantId",
      "itemId",
      "messageAnchors",
      "actualModel",
      "outputFiles",
      "commandsRevision",
      "error",
    ];
    const replayPatch: Partial<Thread> = {};
    function copyReplayField<K extends keyof Thread>(key: K, value: Thread[K]): void {
      replayPatch[key] = value;
    }
    for (const key of eventFields) {
      copyReplayField(key, replay[key]);
    }
    const records = [...(replay.promptRecords ?? [])];
    for (const record of latest.promptRecords ?? []) {
      if (!records.some((item) => item.id === record.id)) {
        records.push(record);
      }
    }
    this.updateThread(
      id,
      {
        ...replayPatch,
        promptRecords: records,
        state,
        capabilities: current.capabilities,
        ...projectionMetadata(thread, latest, snapshot, archived),
      },
      false,
    );
    if (projection.queue !== undefined) {
      this.updateThread(
        id,
        {
          queue: projection.queue.items,
          queuePaused: projection.queue.paused,
        },
        false,
      );
    }
  }
  async ensureSession(id: string): Promise<Thread> {
    const existing = this.launching.get(id);
    if (existing) {
      return await existing;
    }
    const pending = this.ensureSessionInner(id).finally(() => {
      this.launching.delete(id);
    });
    this.launching.set(id, pending);
    return await pending;
  }
  private async ensureSessionInner(id: string) {
    const defaults = this.defaultsPending.get(id);
    if (defaults) {
      await defaults;
      this.defaultsPending.delete(id);
    }
    let thread = this.requireThread(id);

    if (!thread.worktreeId) {
      if (thread.projectId) {
        throw new Error("The project folder is unavailable.");
      }
      const path = await this.resolveChatDirectory();
      this.updateThread(
        id,
        {
          worktreeId: path,
        },
        false,
      );
      thread = this.requireThread(id);
    }
    if (!hasText(thread.sessionId)) {
      if (this.snapshot.disabledHarnesses.includes(thread.harness)) {
        throw new Error(
          "This agent tool is disabled. Enable it in Settings → Agent tools or choose another agent.",
        );
      }
      const session = await this.rpc("client/sessionCreate", launchIntent(thread));
      this.updateThread(id, {
        worktreeId: session.options.cwd,
        sessionId: session.id,
        capabilities: session.capabilities,
        executionPolicy: session.executionPolicy,
        state: session.state ?? "idle",
      });
    } else if (["closed", "failed"].includes(thread.state)) {
      const session = await this.resumeSession(thread);
      this.updateThread(id, {
        state: session.state,
        capabilities: session.capabilities,
        executionPolicy: session.executionPolicy,
        error: undefined,
      });
    }
    thread = this.requireThread(id);
    return thread;
  }
  private async resumeSession(thread: Thread) {
    const result = await this.rpc("client/sessionOpen", {
      sessionId: thread.sessionId,
      resume: "explicit",
    });
    return result.snapshot;
  }
  private async bindPromptRecords(thread: Thread, events: Event[]) {
    const records = (thread.promptRecords ?? []).map((record) => ({
      ...record,
    }));
    const matching = events.filter(
      (event) =>
        event.type === "message.user" &&
        event.sessionId === thread.sessionId &&
        !records.some((record) => record.nativeSequence === event.sequence),
    );
    const keyed = await Promise.all(
      matching.map(async (event) => {
        const content = isRecord(event.data) ? event.data.content : undefined;
        return {
          event,
          key: Array.isArray(content)
            ? await promptKey(decodeContract("Prompt", { content }).content)
            : undefined,
        };
      }),
    );
    for (const { event, key } of keyed) {
      if (key === undefined) {
        continue;
      }
      const record = records.find(
        (item) =>
          item.nativeSequence === undefined &&
          item.afterSequence < event.sequence &&
          item.key === key,
      );
      if (record) {
        record.nativeSequence = event.sequence;
      }
    }
    const live = this.snapshot.threads.find((item) => item.id === thread.id)?.promptRecords ?? [];
    for (const record of live) {
      if (!records.some((item) => item.id === record.id)) {
        records.push(record);
      }
    }
    return {
      ...thread,
      promptRecords: records,
    };
  }
  async send(
    id: string,
    text: string,
    attachments: Attachment[] = [],
    delivery: "prompt" | "steer" | "queue" = "prompt",
  ) {
    if (text.trim().length === 0 && attachments.length === 0) {
      return;
    }
    const thread = await this.ensureSession(id);
    const contextItems = thread.contextItems ?? [];
    const record = {
      id: crypto.randomUUID(),
      key: "",
      afterSequence: thread.sequence,
      text,
      attachments: attachments.map((attachment) => ({ ...attachment, data: "" })),
      contextItems,
    };
    const requestKey = delivery === "prompt" ? crypto.randomUUID() : undefined;
    const before = this.requireThread(id);
    this.updateThread(id, {
      promptRecords: [...(before.promptRecords ?? []), record],
      ...(requestKey === undefined
        ? {}
        : {
            state: "running",
            error: undefined,
            turnId: undefined,
            submissionKeys: [
              ...(before.submissionKeys ?? []),
              { key: requestKey, recordId: record.id },
            ],
          }),
    });
    let attempted = false;
    try {
      await this.persistSubmissionIntent(requestKey);
      attempted = true;
      const result = await this.rpc("client/sessionInput", {
        sessionId: thread.sessionId,
        text,
        attachments,
        contextItems,
        delivery,
        ...(requestKey === undefined ? {} : { idempotencyKey: requestKey }),
      });
      if (result.input) {
        await this.applyInputRecord(id, record.id, result.input);
      }
      this.acknowledgeSubmission(id, requestKey, result.receipt);
      this.settleInputDraft(id, thread, text, attachments, contextItems, result);
    } catch (error) {
      if (isStaleConnectionResponse(error)) {
        throw error;
      }
      if (requestKey !== undefined) {
        await this.recoverFailedSubmission(id, thread, requestKey, attempted, error);
      }
      throw error;
    }
  }
  private async persistSubmissionIntent(key: string | undefined) {
    if (key === undefined) {
      return;
    }
    // Retain view mappings before submitting; native storage owns execution intent.
    this.persist(true);
    if (!nativeClient()) {
      return;
    }
    await this.cacheWriter;
    await this.requireCache().save({
      threads: this.snapshot.threads,
      revision: this.revision,
      metadata: {
        projects: this.snapshot.projects,
        activeId: this.snapshot.activeId,
        customChatDirectory: this.snapshot.customChatDirectory,
      },
    });
  }
  private async applyInputRecord(id: string, recordId: string, input: PreparedInput) {
    const key = await promptKey(input.content);
    const current = this.requireThread(id);
    this.updateThread(id, {
      promptRecords: current.promptRecords?.map((item) =>
        item.id === recordId
          ? { ...item, id: input.id ?? item.id, key, attachments: input.attachments }
          : item,
      ),
    });
  }
  private acknowledgeSubmission(id: string, key: string | undefined, receipt?: SubmissionReceipt) {
    if (key === undefined) {
      return;
    }
    if (!receipt) {
      throw new Error("Delivery confirmation is missing.");
    }
    this.recordSubmissionReceipt(id, key, receipt);
    if (receipt.delivery === "rejected") {
      throw new RpcError(
        nonEmptyText(receipt.error) ?? "The harness rejected this input without effects.",
        -32_001,
        { effectKnowledge: "none" },
      );
    }
    if (receipt.delivery !== "accepted" || receipt.recoveryRequired) {
      throw new Error(
        nonEmptyText(receipt.error) ??
          "Delivery confirmation is missing. Check this session before starting more work.",
      );
    }
  }
  private settleInputDraft(
    id: string,
    previous: Thread,
    text: string,
    attachments: Attachment[],
    contextItems: ContextItem[],
    result: NativeClientMethods["client/sessionInput"]["result"],
  ) {
    const current = this.requireThread(id);
    this.updateThread(id, {
      submitted: true,
      draft: current.draft === text ? "" : current.draft,
      attachments: current.attachments?.filter(
        (attachment) => !attachments.some((sent) => sent.id === attachment.id),
      ),
      contextItems: current.contextItems?.filter(
        (item) => !contextItems.some((sent) => sent.id === item.id),
      ),
      ...(result.queue ? { queue: result.queue.items, queuePaused: result.queue.paused } : {}),
    });
    if (hasText(result.title) && current.title === previous.title) {
      this.updateThread(id, { title: result.title });
    }
    if (
      result.metadataError !== undefined &&
      result.metadataError !== null &&
      result.metadataError !== false &&
      result.metadataError !== 0 &&
      result.metadataError !== ""
    ) {
      this.setError("The input was accepted, but its chat title could not be saved.");
    }
  }
  private async recoverFailedSubmission(
    id: string,
    previous: Thread,
    key: string,
    attempted: boolean,
    error: unknown,
  ) {
    if (
      !attempted ||
      (error instanceof RpcError && isRecord(error.data) && error.data.effectKnowledge === "none")
    ) {
      const latest = this.requireThread(id);
      this.updateThread(id, {
        state: previous.state,
        submissionKeys: latest.submissionKeys?.filter((entry) => entry.key !== key),
      });
      return;
    }
    this.updateThread(id, {
      state: "needs_input",
      error: "Delivery confirmation is missing. Reconnect to check the original submission.",
    });
    try {
      await this.recoverSubmissions(id);
    } catch {
      this.updateThread(
        id,
        {
          state: "needs_input",
          error: "Delivery confirmation is missing. Reconnect to recover the original submission.",
        },
        false,
      );
    }
  }
  private recordSubmissionReceipt(id: string, key: string, receipt: SubmissionReceipt) {
    const thread = this.requireThread(id);
    this.updateThread(
      id,
      {
        submissionKeys: thread.submissionKeys?.map((entry) =>
          entry.key === key
            ? {
                ...entry,
                receipt,
              }
            : entry,
        ),
      },
      false,
    );
    this.persist();
  }
  async recoverSubmissions(
    id: string,
    reconcile = false,
    retainedProjection?: SubmissionProjection,
  ) {
    const thread = this.snapshot.threads.find((item) => item.id === id);
    if (!hasText(thread?.sessionId)) {
      return;
    }
    const projection =
      retainedProjection ??
      (await this.rpc("client/submissions", { sessionId: thread.sessionId, reconcile }));
    const latest = this.snapshot.threads.find((item) => item.id === id);
    if (!latest || latest.sessionId !== thread.sessionId) {
      return;
    }
    const records = await recoveredPromptRecords(latest, projection);
    const current = this.snapshot.threads.find((item) => item.id === id);
    if (!current || current.sessionId !== thread.sessionId) {
      return;
    }
    for (const retained of current.promptRecords ?? []) {
      if (!records.some((item) => item.id === retained.id)) {
        records.push(retained);
      }
    }
    this.updateThread(id, { promptRecords: records }, false);
    const entries = projection.entries.map((entry) => ({
      key: entry.key,
      recordId:
        current.submissionKeys?.find((item) => item.key === entry.key)?.recordId ?? entry.key,
      receipt: entry.receipt,
    }));
    this.updateThread(
      id,
      {
        submissionKeys: entries,
        ...(projection.unresolved
          ? {
              state: "needs_input",
              error:
                "Delivery confirmation is missing. Check the original submission before sending more work.",
            }
          : {
              state:
                current.sequence > projection.snapshot.sequence
                  ? current.state
                  : projection.snapshot.state,
              turnId:
                current.sequence > projection.snapshot.sequence
                  ? current.turnId
                  : (projection.snapshot.turn ?? undefined),
            }),
      },
      false,
    );
    for (const entry of entries) {
      this.applyRecoveredDraft(id, entry, projection.unresolved);
    }
    this.persist();
  }
  private applyRecoveredDraft(
    id: string,
    entry: { recordId: string; receipt: SubmissionReceipt },
    unresolved: boolean,
  ) {
    if (entry.receipt.delivery !== "accepted" || entry.receipt.recoveryRequired) {
      return;
    }
    // Only presentation fields are reconciled; native storage owns delivery knowledge.
    const current = this.requireThread(id);
    const record = current.promptRecords?.find((item) => item.id === entry.recordId);
    this.updateThread(id, {
      submitted: true,
      ...(unresolved ? {} : { error: undefined }),
      ...(record
        ? {
            draft: current.draft === record.text ? "" : current.draft,
            attachments: current.attachments?.filter(
              (item) => !record.attachments.some((sent) => sent.id === item.id),
            ),
            contextItems: current.contextItems?.filter(
              (item) => !record.contextItems.some((sent) => sent.id === item.id),
            ),
          }
        : {}),
    });
  }
  async checkSubmissionRecovery(id: string) {
    const thread = this.snapshot.threads.find((item) => item.id === id);
    if (!hasText(thread?.sessionId)) {
      return;
    }
    try {
      const result = await this.rpc("client/sessionRecover", {
        sessionId: thread.sessionId,
      });
      await this.applyProjection(id, result);
    } catch (error) {
      this.updateThread(id, {
        state: "needs_input",
        error: error instanceof Error ? error.message : String(error),
      });
      throw error;
    }
  }
  private async applyQueue(
    id: string,
    queue: {
      items: Queued[];
      paused: boolean;
      input?: PreparedInput;
    },
  ) {
    this.updateThread(
      id,
      {
        queue: queue.items,
        queuePaused: queue.paused,
      },
      false,
    );
    if (hasText(queue.input?.id)) {
      const { input } = queue;
      const current = this.requireThread(id);
      const key = await promptKey(input.content);
      this.updateThread(id, {
        promptRecords: current.promptRecords?.map((record) =>
          record.id === input.id
            ? {
                ...record,
                text: input.text,
                key,
                attachments: input.attachments,
                contextItems: input.contextItems,
              }
            : record,
        ),
      });
    }
  }
  async refreshQueue(id: string) {
    const thread = this.requireThread(id);
    await this.applyQueue(
      id,
      await this.rpc("client/sessionQueue", {
        sessionId: thread.sessionId,
        action: "list",
      }),
    );
  }
  async editQueue(id: string, itemId: string, text?: string) {
    const thread = this.requireThread(id);
    await this.applyQueue(
      id,
      await this.rpc("client/sessionQueue", {
        sessionId: thread.sessionId,
        id: itemId,
        action: text === undefined ? "remove" : "update",
        ...(text === undefined
          ? {}
          : {
              text,
            }),
      }),
    );
  }
  async pauseQueue(id: string, paused: boolean) {
    const thread = this.requireThread(id);
    await this.applyQueue(
      id,
      await this.rpc("client/sessionQueue", {
        sessionId: thread.sessionId,
        action: "pause",
        paused,
      }),
    );
  }
  async rename(id: string, title: string) {
    const thread = this.requireThread(id);
    if (!title.trim()) {
      return;
    }
    if (hasText(thread.sessionId)) {
      await this.rpc("session/metadata", {
        sessionId: thread.sessionId,
        title: title.trim(),
      });
    }
    this.updateThread(id, {
      title: title.trim(),
    });
  }
  async archive(id: string, archived: boolean) {
    const thread = this.requireThread(id);
    if (hasText(thread.sessionId)) {
      await this.rpc("session/metadata", {
        sessionId: thread.sessionId,
        archived,
      });
    }
    this.updateThread(id, {
      archived,
    });
  }
  async loadControls(id: string) {
    const thread = await this.ensureSession(id);
    this.applyControls(
      id,
      await this.rpc("client/sessionControls", {
        sessionId: thread.sessionId,
      }),
    );
  }
  async previewControls(id: string) {
    const defaults = this.defaultsPending.get(id);
    if (defaults) {
      await defaults;
    }
    const thread = this.snapshot.threads.find((item) => item.id === id);
    if (
      !thread ||
      !this.snapshot.connected ||
      !thread.worktreeId ||
      (!hasText(thread.sessionId) && this.snapshot.disabledHarnesses.includes(thread.harness))
    ) {
      return;
    }
    if (hasText(thread.sessionId)) {
      await this.loadControls(id);
      return;
    }
    const directory = previewDirectory(thread);
    const options = {
      harness: thread.harness,
      ...directory,
      model: thread.model || null,
      config: thread.config ?? {},
    };
    const key = `${id}:${JSON.stringify(options)}`;
    const existing = this.controlPreviews.get(key);
    if (existing) {
      await existing;
      return;
    }
    const pending = (async () => {
      const result = await this.rpc("client/sessionPrepare", launchIntent(thread));
      const current = this.snapshot.threads.find((item) => item.id === id);
      if (
        !current ||
        hasText(current.sessionId) ||
        current.harness !== options.harness ||
        current.worktreeId !== thread.worktreeId ||
        (current.model || null) !== options.model ||
        JSON.stringify(current.config ?? {}) !== JSON.stringify(options.config)
      ) {
        return;
      }
      this.updateThread(
        id,
        {
          capabilities: result.capabilities,
          configurations: result.configuration ?? undefined,
          executionPolicy: result.executionPolicy,
          models: result.models ?? undefined,
        },
        false,
      );
    })().finally(() => {
      this.controlPreviews.delete(key);
    });
    this.controlPreviews.set(key, pending);
    await pending;
  }
  async configure(id: string, key: string, value: unknown) {
    await this.configureMany(id, {
      [key]: value,
    });
  }
  async configureMany(id: string, config: Record<string, unknown>) {
    const thread = this.requireThread(id);
    if (!hasText(thread.sessionId)) {
      const values = {
        ...thread.config,
        ...config,
      };
      if (
        !("executionProfile" in config) &&
        Object.keys(config).some(
          (key) => thread.executionPolicy?.controlledSettings.includes(key) === true,
        )
      ) {
        delete values.executionProfile;
      }
      this.updateThread(id, {
        config: values,
        ...("model" in config
          ? {
              model: String(config.model),
            }
          : {}),
      });
      return;
    }
    try {
      const receipt = await this.rpc("client/sessionConfigure", {
        sessionId: thread.sessionId,
        config,
      });
      const values = {
        ...thread.config,
        ...receipt.values,
      };
      if (!receipt.executionPolicy.requested) {
        delete values.executionProfile;
      }
      this.updateThread(id, {
        config: values,
        configurations: receipt.configuration,
        executionPolicy: receipt.executionPolicy,
        ...("model" in receipt.values
          ? {
              model: typeof receipt.values.model === "string" ? receipt.values.model : "",
            }
          : {}),
      });
    } catch (error) {
      // Reconcile observed state after a partially applied native update.
      const controls =
        error instanceof RpcError && isRecord(error.data) && error.data.controls !== undefined
          ? decodeClientResult("client/sessionControls", error.data.controls)
          : undefined;
      if (controls) {
        this.applyControls(id, controls);
      }
      throw error;
    }
  }
  private applyControls(id: string, controls: SessionControls) {
    const thread = this.snapshot.threads.find((item) => item.id === id);
    if (!thread) {
      return;
    }
    const config = {
      ...thread.config,
      ...controls.values,
    };
    if (!controls.executionPolicy.requested) {
      delete config.executionProfile;
    }
    this.updateThread(
      id,
      {
        configurations: controls.configuration,
        models: controls.models,
        executionPolicy: controls.executionPolicy,
        config,
        ...("model" in controls.values
          ? {
              model: typeof controls.values.model === "string" ? controls.values.model : "",
            }
          : {}),
      },
      false,
    );
  }
  async tasks(id: string): Promise<AgentTask[]> {
    const thread = this.snapshot.threads.find((candidate) => candidate.id === id);
    if (
      !hasText(thread?.sessionId) ||
      thread.capabilities?.backgroundTasks?.availability !== "available"
    ) {
      throw new Error("Background tasks are unavailable for this chat");
    }
    return await this.rpc("session/tasks", {
      sessionId: thread.sessionId,
    });
  }
  async stopTask(id: string, taskId: string) {
    const thread = this.snapshot.threads.find((candidate) => candidate.id === id);
    if (
      !hasText(thread?.sessionId) ||
      thread.capabilities?.backgroundTasks?.availability !== "available"
    ) {
      throw new Error("Background task controls are unavailable for this chat");
    }
    await this.rpc("session/task/stop", {
      sessionId: thread.sessionId,
      taskId,
    });
  }
  async compact(id: string) {
    const thread = await this.ensureSession(id);
    await this.rpc("session/compact", {
      sessionId: thread.sessionId,
    });
  }
  private async changeHistory(id: string, intent: Record<string, unknown>) {
    const thread = this.requireThread(id);
    return await this.rpc("client/sessionHistory", {
      sessionId: thread.sessionId,
      ...intent,
    });
  }
  private async showBranch(
    id: string,
    result: Awaited<ReturnType<WorkspaceStore["changeHistory"]>>,
  ) {
    const thread = this.requireThread(id);
    const { snapshot } = result.projection;
    const child = {
      ...newThread(thread.projectId, snapshot.options.cwd, snapshot.options.harness),
      sessionId: snapshot.id,
      title: `${thread.title} · fork`,
      model: snapshot.options.model ?? "",
      config: isRecord(snapshot.options.config) ? snapshot.options.config : {},
    };
    this.publish({
      threads: [child, ...this.snapshot.threads],
      activeId: child.id,
    });
    await this.applyProjection(child.id, result.projection);
    if (result.draft) {
      this.updateThread(child.id, result.draft);
    }
    return child.id;
  }
  async fork(id: string, messageId?: string, position?: "before_user" | "after_assistant") {
    return await this.showBranch(
      id,
      await this.changeHistory(id, {
        action: "fork",
        ...(hasText(messageId)
          ? {
              messageId,
              position,
            }
          : {}),
      }),
    );
  }
  async editInBranch(
    id: string,
    node: {
      id: string;
      text: string;
      metadata?: {
        prompt?: {
          content: PromptPart[];
        };
      };
    },
  ) {
    const child = await this.showBranch(
      id,
      await this.changeHistory(id, {
        action: "branch",
        messageId: node.id,
      }),
    );
    window.dispatchEvent(
      new CustomEvent("swarmie:composer-text", {
        detail: {
          threadId: child,
        },
      }),
    );
    return child;
  }
  async editFrom(
    id: string,
    messageId: string,
    scope: "conversation_only" | "conversation_and_files" = "conversation_only",
  ) {
    const result = await this.changeHistory(id, {
      action: "edit",
      messageId,
      scope,
    });
    await this.applyProjection(id, result.projection);
    if (result.draft) {
      this.updateThread(id, result.draft);
    }
    window.dispatchEvent(
      new CustomEvent("swarmie:composer-text", {
        detail: {
          threadId: id,
        },
      }),
    );
  }
  async rollback(
    id: string,
    turns: number,
    scope: "conversation_only" | "conversation_and_files" = "conversation_only",
  ) {
    const result = await this.changeHistory(id, {
      action: "rollback",
      turns,
      scope,
    });
    await this.applyProjection(id, result.projection);
  }
  invokeExtension(extensionId: string, command: string, args: unknown): Promise<unknown>;
  invokeExtension<T>(
    extensionId: string,
    command: string,
    args: unknown,
    decode: (value: unknown) => T,
  ): Promise<T>;
  async invokeExtension(
    extensionId: string,
    command: string,
    args: unknown,
    decode?: (value: unknown) => unknown,
  ): Promise<unknown> {
    const value = await this.rpc("client/extensionInvoke", {
      extensionId,
      command,
      arguments: args,
    });
    return decode ? decode(value) : value;
  }
  async refreshExtensions() {
    this.publish({
      extensions: await this.rpc("extension/list"),
    });
  }
  async cancel(id: string) {
    const thread = this.snapshot.threads.find((t) => t.id === id);
    if (hasText(thread?.sessionId)) {
      await this.rpc("session/cancel", {
        sessionId: thread.sessionId,
        ...(hasText(thread.turnId)
          ? {
              turnId: thread.turnId,
            }
          : {}),
      });
    }
  }
  async close(id: string) {
    const thread = this.snapshot.threads.find((t) => t.id === id);
    if (hasText(thread?.sessionId) && thread.state !== "closed") {
      await this.rpc("session/close", {
        sessionId: thread.sessionId,
      });
    }
    if (this.snapshot.threads.find((t) => t.id === id)?.sessionId === thread?.sessionId) {
      this.updateThread(id, {
        state: "closed",
        permissions: [],
      });
    }
  }
  async respond(id: string, option: string, data: unknown) {
    const thread = this.active();
    if (!hasText(thread?.sessionId)) {
      return;
    }
    await this.rpc("permission/respond", {
      sessionId: thread.sessionId,
      id,
      option,
      data,
    });
    this.updateThread(thread.id, {
      permissions: thread.permissions.filter((p) => p.id !== id),
    });
  }
  async review() {
    const thread = this.active();
    if (
      !hasText(thread?.projectId) ||
      !(this.snapshot.projects.find((p) => p.id === thread.projectId)?.git === true)
    ) {
      return;
    }
    const review = await this.extension(
      "review",
      {
        projectId: thread.projectId,
        worktreeId: thread.worktreeId,
      },
      decodeReviewResult,
    );
    if (this.snapshot.activeId === thread.id) {
      this.publish({
        review,
      });
    }
  }
  async createWorktree(branch: string) {
    const thread = this.active();
    if (!hasText(thread?.projectId)) {
      throw new Error("Open a project first.");
    }
    if (hasText(thread.sessionId)) {
      throw new Error("Start a new chat before changing its worktree.");
    }
    {
      const result = await this.rpc("client/worktree", {
        projectId: thread.projectId,
        action: "create",
        branch,
      });
      this.publish({
        projects: this.snapshot.projects.map((p) =>
          p.id === thread.projectId
            ? {
                ...p,
                worktrees: result.worktrees,
              }
            : p,
        ),
        review: undefined,
      });
      if (result.tree === null) {
        throw new Error("The runtime did not return the created worktree.");
      }
      this.updateThread(thread.id, {
        worktreeId: result.tree.id,
      });
      if (result.tree.setup?.completed === false) {
        this.setError(
          `Worktree created, but setup failed: ${result.tree.setup.message ?? "Open Worktree setup to retry."}`,
        );
      }
    }
  }
  async refreshProject(id: string) {
    const project = this.snapshot.projects.find((p) => p.id === id);
    if (!project) {
      return;
    }
    const opened = await this.rpc("client/projectGet", {
      projectId: id,
    });
    this.publish({
      projects: this.snapshot.projects.map((p) => (p.id === id ? opened : p)),
    });
  }
  async removeWorktree() {
    const thread = this.active();
    if (!thread || hasText(thread.sessionId)) {
      throw new Error("Start a new chat before removing its worktree.");
    }
    {
      const result = await this.rpc("client/worktree", {
        projectId: thread.projectId,
        action: "remove",
        worktreeId: thread.worktreeId,
      });
      this.publish({
        projects: this.snapshot.projects.map((p) =>
          p.id === thread.projectId
            ? {
                ...p,
                worktrees: result.worktrees,
              }
            : p,
        ),
        review: undefined,
      });
      this.updateThread(thread.id, {
        worktreeId: result.worktrees[0]?.id ?? "",
      });
    }
  }
  private async reviewAfterTurn() {
    try {
      await this.review();
    } catch (error) {
      this.setError(error);
    }
  }
  private async pollThread(thread: Thread): Promise<boolean> {
    try {
      if (
        thread.submissionKeys?.some(
          (entry) =>
            !entry.receipt ||
            entry.receipt.recoveryRequired ||
            !["accepted", "rejected"].includes(entry.receipt.delivery),
        ) === true
      ) {
        await this.recoverSubmissions(thread.id);
      }
      const events = await this.rpc("session/events", {
        sessionId: thread.sessionId,
        afterSequence: thread.sequence,
        limit: 256,
      });
      if (!events.length) {
        return true;
      }
      const current = this.requireThread(thread.id);
      if (
        current.sessionId !== thread.sessionId ||
        ["closed", "starting"].includes(current.state)
      ) {
        return true;
      }
      const bound = await this.bindPromptRecords(current, nativeEvents(events));
      const latest = this.requireThread(current.id);
      if (latest.sessionId !== current.sessionId) {
        return true;
      }
      const next = reduceEvents(
        {
          ...latest,
          promptRecords: bound.promptRecords,
        },
        nativeEvents(events),
      );
      if (typeof document !== "undefined") {
        try {
          this.notificationService.events(
            next,
            nativeEvents(events),
            this.snapshot.notifications,
            this.requireNode(),
          );
        } catch (error) {
          this.setError(new Error(`Desktop notification failed: ${String(error)}`));
        }
      }
      this.publish({
        threads: this.snapshot.threads.map((t) => (t.id === next.id ? next : t)),
      });
      if (events.some((e) => e.type === "turn.ended") && next.id === this.snapshot.activeId) {
        await this.reviewAfterTurn();
      }
    } catch (error) {
      if (!this.snapshot.connected || isRuntimeUnavailable(error)) {
        return false;
      }
      const current = this.snapshot.threads.find((t) => t.id === thread.id);
      if (
        !current ||
        current.sessionId !== thread.sessionId ||
        ["closed", "starting"].includes(current.state)
      ) {
        return true;
      }
      this.updateThread(thread.id, {
        state: "failed",
        error: error instanceof Error ? error.message : String(error),
        permissions: [],
      });
    }
    return true;
  }
  async poll() {
    if (this.polling || this.connection) {
      return;
    }
    if (!this.snapshot.connected) {
      if (Date.now() >= this.reconnectAt) {
        await this.connect();
      }
      return;
    }
    this.polling = true;
    try {
      if (!nativeClient() && this.runtimeIdentity && Date.now() >= this.heartbeatAt) {
        await this.rpc("runtime/initialize");
        this.heartbeatAt = Date.now() + 5000;
      }
      for (const thread of this.snapshot.threads.filter(
        (t) => hasText(t.sessionId) && !["closed", "failed", "starting"].includes(t.state),
      )) {
        // oxlint-disable-next-line eslint/no-await-in-loop -- Session cursors and disconnection ownership are applied serially; a failed route stops later session reads.
        if (!(await this.pollThread(thread))) {
          break;
        }
      }
    } catch (error) {
      if (this.snapshot.connected) {
        this.setError(error);
      }
    } finally {
      this.polling = false;
    }
  }
}

function launchIntent(thread: Thread) {
  return {
    harness: thread.harness,
    model: launchModel(thread),
    config: thread.config ?? {},
    ...(thread.projectId
      ? {
          projectId: thread.projectId,
          projectRevision: thread.projectRevision,
          directoryPath: thread.worktreeId,
        }
      : {}),
  };
}

function launchModel(thread: Thread): string | null {
  const configured = thread.configurations?.find((option) => option.id === "model")?.value;
  return (
    nonEmptyText(thread.model) ??
    (typeof configured === "string" ? nonEmptyText(configured) : undefined) ??
    null
  );
}

async function recoveredPromptRecords(latest: Thread, projection: SubmissionProjection) {
  const records = [...(latest.promptRecords ?? [])];
  const inputs = projection.entries.flatMap((entry) =>
    entry.input ? [{ entry, input: entry.input }] : [],
  );
  const keyed = await Promise.all(
    inputs.map(async ({ entry, input }) => ({ entry, input, key: await promptKey(input.content) })),
  );
  for (const { entry, input, key } of keyed) {
    const existing =
      latest.submissionKeys?.find((item) => item.key === entry.key)?.recordId ?? entry.key;
    const record = records.find((item) => item.id === existing);
    if (record) {
      record.key = key;
      record.attachments = input.attachments;
    } else {
      records.push({
        id: existing,
        key,
        afterSequence: input.afterSequence,
        text: input.text,
        attachments: input.attachments,
        contextItems: input.contextItems,
      });
    }
  }
  const authored = (projection.inputs ?? []).flatMap((input) =>
    hasText(input.id) ? [{ input, id: input.id }] : [],
  );
  const keys = await Promise.all(
    authored.map(async ({ input, id }) => ({ input, id, key: await promptKey(input.content) })),
  );
  for (const { input, id, key } of keys) {
    const record = {
      id,
      key,
      afterSequence: input.afterSequence,
      text: input.text,
      attachments: input.attachments,
      contextItems: input.contextItems,
    };
    const index = records.findIndex((item) => item.id === id);
    if (index === -1) {
      records.push(record);
    } else {
      records[index] = { ...records[index], ...record };
    }
  }
  return records;
}

function previewDirectory(thread: Thread) {
  if (thread.projectId.length === 0) {
    return {};
  }
  if (thread.projectDirectory === thread.worktreeId) {
    return { projectId: thread.projectId, projectRevision: thread.projectRevision };
  }
  return { cwd: thread.worktreeId };
}

function replayThread(thread: Thread, promptRecords: Thread["promptRecords"]): Thread {
  return {
    ...thread,
    messages: [],
    tools: [],
    tasks: [],
    permissions: [],
    sequence: 0,
    reasoning: undefined,
    plan: undefined,
    subagents: [],
    usage: undefined,
    context: undefined,
    error: undefined,
    queue: [],
    queuePaused: false,
    turnId: undefined,
    turnGroups: [],
    assistantId: undefined,
    itemId: undefined,
    messageAnchors: {},
    promptRecords,
  } satisfies Thread;
}

function projectionMetadata(
  thread: Thread,
  latest: Thread,
  snapshot: SessionSnapshot,
  archived: Thread["archived"],
) {
  return {
    title:
      latest.title === thread.title
        ? (nonEmptyText(snapshot.metadata.title) ?? thread.title)
        : latest.title,
    archived: latest.archived === thread.archived ? archived : latest.archived,
    turnId: snapshot.turn ?? undefined,
    createdAtMs: snapshot.metadata.createdAtMs || thread.createdAtMs,
    updatedAtMs:
      latest.updatedAtMs === thread.updatedAtMs
        ? snapshot.metadata.updatedAtMs || thread.updatedAtMs
        : latest.updatedAtMs,
  };
}
