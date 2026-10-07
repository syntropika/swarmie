import {
  NotificationService,
  readNotificationPreferences,
  type NotificationPreferences,
} from "./notifications";
import { WorkspaceCache, hydrateThreads } from "./cache";
import { fingerprint } from "./reviewed";
import {
  newThread,
  reduceEvents,
  type Thread,
  type Project,
  type Event,
  type Detection,
  type Review,
  type Capability,
  type SessionSnapshot,
  type ExtensionInfo,
  type Configuration,
  type Attachment,
  type PromptPart,
  type Queued,
} from "./protocol";
import {
  contextText,
  lineExcerpt,
  type ContextItem,
  type FileLocation,
} from "./context";

export type Workspace = {
  projects: Project[];
  threads: Thread[];
  activeId: string;
  harnesses: Detection[];
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
export class RpcError extends Error {
  constructor(
    message: string,
    public readonly code: number,
  ) {
    super(message);
  }
}
const storageKey = "mpai.workspace.v2";
function mergeConfiguration(
  defaults: Record<string, unknown>,
  overrides: Record<string, unknown>,
): Record<string, unknown> {
  const result = { ...defaults };
  for (const [key, value] of Object.entries(overrides)) {
    const base = defaults[key];
    const merged =
      value &&
      base &&
      typeof value === "object" &&
      typeof base === "object" &&
      !Array.isArray(value) &&
      !Array.isArray(base)
        ? mergeConfiguration(
            base as Record<string, unknown>,
            value as Record<string, unknown>,
          )
        : value;
    Object.defineProperty(result, key, {
      value: merged,
      enumerable: true,
      configurable: true,
      writable: true,
    });
  }
  return result;
}
function promptKey(content: PromptPart[]) {
  return fingerprint(
    JSON.stringify(
      content.map((part) => {
        if (part.type === "image")
          return [part.type, part.data.mime_type, part.data.data];
        if (part.type === "skill")
          return [part.type, part.data.name, part.data.path];
        return [part.type, part.data];
      }),
    ),
  );
}

export class WorkspaceStore {
  private listeners = new Set<Listener>();
  private polling = false;
  private requestId = 0;
  private token = "";
  private notificationService = new NotificationService();
  private cache = new WorkspaceCache();
  private edited = new Map<string, Set<keyof Thread>>();
  private configurationEdited = new Set<string>();
  private defaultsPending = new Map<string, Promise<void>>();
  private launching = new Map<string, Promise<Thread>>();
  private controlPreviews = new Map<string, Promise<void>>();
  private messageTimes = new Map<string, Promise<void>>();
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
  subscribe = (listener: Listener) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };
  constructor() {
    try {
      const saved = JSON.parse(localStorage.getItem(storageKey) ?? "null");
      if (
        saved?.version === 2 &&
        Array.isArray(saved.projects) &&
        Array.isArray(saved.threads)
      ) {
        this.snapshot = {
          ...this.snapshot,
          projects: saved.projects,
          threads: saved.threads,
          activeId: saved.activeId,
          customChatDirectory: typeof saved.customChatDirectory === "string" ? saved.customChatDirectory : undefined,
        };
      }
    } catch {
      /* Corrupt browser storage must not prevent startup. */
    }
    this.hydration = this.cache
      .load()
      .then((saved) => {
        if (saved)
          this.snapshot = {
            ...this.snapshot,
            threads: hydrateThreads(
              this.snapshot.threads,
              saved.threads,
              this.edited,
            ),
          };
        this.listeners.forEach((listener) => listener());
      })
      .catch((error) => this.storageError(error));
    if (typeof window !== "undefined")
      window.addEventListener("pagehide", () => this.persist());
  }
  private publish(patch: Partial<Workspace>) {
    this.snapshot = { ...this.snapshot, ...patch };
    this.listeners.forEach((listener) => listener());
    clearTimeout(this.saveTimer);
    this.saveTimer = setTimeout(() => this.persist(), 500);
  }
  private storageError(error: unknown) {
    this.snapshot = {
      ...this.snapshot,
      error: `Draft storage failed: ${error instanceof Error ? error.message : String(error)}. Keep this window open and free disk space before closing.`,
    };
    this.listeners.forEach((listener) => listener());
  }
  private persist() {
    this.revision++;
    try {
      const { projects, threads, activeId, customChatDirectory } = this.snapshot;
      localStorage.setItem(
        storageKey,
        JSON.stringify({
          version: 2,
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
    }
    if (!this.cache.available()) {
      if (
        this.snapshot.threads.some(
          (thread) =>
            thread.attachments?.length ||
            Object.keys(thread.fileDrafts ?? {}).length ||
            thread.promptRecords?.length,
        )
      )
        this.storageError("Persistent attachment storage is unavailable");
      return;
    }
    this.cachePending = true;
    if (this.cacheWriter) return;
    this.snapshot = { ...this.snapshot, cacheSaving: true };
    this.listeners.forEach((listener) => listener());
    this.cacheWriter = this.writeCache();
  }
  private async writeCache() {
    await this.hydration;
    try {
      while (this.cachePending) {
        this.cachePending = false;
        await this.cache.save({
          threads: this.snapshot.threads,
          revision: this.revision,
        });
      }
    } catch (error) {
      this.storageError(error);
    } finally {
      this.cacheWriter = undefined;
      this.snapshot = { ...this.snapshot, cacheSaving: false };
      this.listeners.forEach((listener) => listener());
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
    for (const key of Object.keys(patch)) fields.add(key as keyof Thread);
    this.edited.set(id, fields);
    if (
      ["harness", "model", "worktreeId"].some(
        (key) => key in patch,
      )
    )
      this.configurationEdited.add(id);
    this.publish({
      threads: this.snapshot.threads.map((t) =>
        t.id === id
          ? { ...t, ...patch, ...(touch ? { updatedAtMs: Date.now() } : {}) }
          : t,
      ),
    });
    if ("draft" in patch || "attachments" in patch) this.persist();
  }
  active() {
    return this.snapshot.threads.find((t) => t.id === this.snapshot.activeId);
  }
  async findFiles(query: string) {
    const thread = this.active();
    if (!thread?.worktreeId) return [];
    await this.openWorkspace(thread);
    const result = await this.invokeExtension<{ paths: string[] }>(
      "workspace",
      "files.find",
      { projectId: thread.worktreeId, query, limit: 80 },
    );
    return result.paths;
  }
  openFile(location: FileLocation) {
    const thread = this.active();
    if (!thread) return;
    const relative = location.path.startsWith(`${thread.worktreeId}/`)
      ? location.path.slice(thread.worktreeId.length + 1)
      : location.path;
    const file = { ...location, path: relative };
    const files = [
      ...(thread.layout?.files ?? []).filter((f) => f.path !== relative),
      file,
    ];
    if (files.length > 8)
      throw new Error(
        "Keep up to eight files open in a conversation. Close a file tab first.",
      );
    this.updateThread(thread.id, {
      layout: {
        ...thread.layout,
        files,
        activeFile: file,
        panel: "extensions",
        selectedView: "workspace:files",
      },
    });
    window.dispatchEvent(new CustomEvent("mpai:open-tools"));
  }
  addContext(id: string, item: Omit<ContextItem, "id">) {
    const thread = this.snapshot.threads.find((t) => t.id === id);
    if (!thread) return;
    const items = [
      ...(thread.contextItems ?? []),
      { ...item, id: crypto.randomUUID() },
    ];
    if (new TextEncoder().encode(contextText(items)).length > 1024 * 1024)
      throw new Error("Keep selected context under 1 MB.");
    this.updateThread(id, { contextItems: items });
  }
  async addFileContext(id: string, location: FileLocation) {
    const thread = this.snapshot.threads.find((t) => t.id === id);
    if (!thread) return;
    await this.openWorkspace(thread);
    const result = await this.invokeExtension<{ content: string }>(
      "workspace",
      "files.read",
      { projectId: thread.worktreeId, path: location.path },
    );
    const text = lineExcerpt(
      result.content,
      location.startLine,
      location.endLine,
    );
    this.addContext(id, {
      kind: "file",
      label: `${location.path}${location.startLine ? `:${location.startLine}-${location.endLine ?? location.startLine}` : ""}`,
      text,
      ...location,
    });
  }
  reorderThread(id: string, before: string) {
    const threads = [...this.snapshot.threads];
    const index = threads.findIndex((t) => t.id === id);
    if (index < 0 || id === before) return;
    const [thread] = threads.splice(index, 1);
    const target = threads.findIndex((t) => t.id === before);
    threads.splice(target < 0 ? threads.length : target, 0, thread);
    this.publish({ threads });
  }
  orderThreads(ids: string[]) {
    const order = new Map(ids.map((id, index) => [id, index]));
    this.publish({
      threads: [...this.snapshot.threads].sort(
        (a, b) =>
          (order.get(a.id) ?? ids.length) - (order.get(b.id) ?? ids.length),
      ),
    });
  }
  select(id: string) {
    this.publish({ activeId: id, review: undefined, error: "" });
  }
  setError(error: unknown) {
    this.publish({
      error: error instanceof Error ? error.message : String(error),
      busy: false,
    });
  }
  dismissError() {
    this.publish({ error: "" });
  }
  async rpc<T>(method: string, params: unknown = {}): Promise<T> {
    const response = await fetch("/rpc", {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        ...(this.token ? { Authorization: `Bearer ${this.token}` } : {}),
      },
      body: JSON.stringify({
        jsonrpc: "2.0",
        id: ++this.requestId,
        method,
        params,
      }),
      signal: AbortSignal.timeout(120_000),
    });
    if (!response.ok)
      throw new Error(
        response.status === 401
          ? "Enter the daemon token in Settings, then reconnect."
          : `The daemon returned HTTP ${response.status}.`,
      );
    const body = await response.json();
    if (body.error) throw new RpcError(body.error.message, body.error.code);
    return body.result as T;
  }
  extension<T>(command: string, args: unknown) {
    return this.rpc<T>("extension/invoke", {
      extensionId: "git",
      command,
      arguments: args,
    });
  }
  async setNotifications(preferences: NotificationPreferences) {
    await this.notificationService.configure(preferences);
    this.publish({ notifications: preferences });
  }
  async probe(harness: string) {
    const result = await this.rpc<{
      authState: string;
      message?: string;
      version?: string;
    }>("runtime/probe", { harness });
    this.publish({
      harnesses: this.snapshot.harnesses.map((h) =>
        h.harness === harness ? { ...h, ...result } : h,
      ),
    });
    return result;
  }
  async connect(token = this.token) {
    await this.hydration;
    this.token = token;
    this.publish({ busy: true, error: "", connected: false });
    try {
      await this.rpc("runtime/initialize");
      const [harnesses, extensions] = await Promise.all([
        this.rpc<Detection[]>("runtime/discover"),
        this.rpc<ExtensionInfo[]>("extension/list"),
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
          const path = await this.resolveChatDirectory(this.snapshot.customChatDirectory);
          for (const thread of this.snapshot.threads.filter((t) => !t.projectId && !t.worktreeId)) {
            this.updateThread(thread.id, { worktreeId: path }, false);
          }
        } catch (error) {
          this.setError(error);
        }
      }
      for (const project of this.snapshot.projects) {
        const opened = await this.openProjectRecord(project.path);
        this.publish({
          projects: this.snapshot.projects.map((p) =>
            p.id === project.id
              ? { ...p, ...opened, name: project.name || opened.name }
              : p,
          ),
        });
      }
      await this.restoreSessions();
      if (!this.snapshot.threads.length) this.createThread();
      for (const thread of this.snapshot.threads.filter((t) => t.sessionId)) {
        try {
          await this.restoreThread(thread.id);
        } catch (error) {
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
    } catch (error) {
      this.setError(error);
    }
  }
  async openProjectRecord(path: string): Promise<Project> {
    if (this.snapshot.git) {
      try {
        const project = await this.extension<Project>("project.open", { path });
        return {
          ...project,
          name: project.name || project.path.split("/").filter(Boolean).at(-1) || project.path,
          git: true,
        };
      } catch (error) {
        if (!this.snapshot.extensions.some((e) => e.id === "workspace"))
          throw error;
      }
    }
    const folder = await this.invokeExtension<{ id: string; path: string }>(
      "workspace",
      "project.open",
      { path },
    );
    return {
      ...folder,
      git: false,
      name: folder.path.split("/").filter(Boolean).at(-1) ?? folder.path,
      worktrees: [
        {
          id: folder.path,
          path: folder.path,
          branch: null,
          isMain: true,
          locked: false,
          prunable: false,
        },
      ],
    };
  }
  async openWorkspace(thread: Thread) {
    const project = this.snapshot.projects.find(
      (p) => p.id === thread.projectId,
    );
    const cwd =
      project?.worktrees.find((w) => w.id === thread.worktreeId)?.path ??
      thread.worktreeId;
    if (!cwd) throw new Error("Open a project first.");
    return this.invokeExtension<{ id: string; path: string }>(
      "workspace",
      "project.open",
      { path: cwd },
    );
  }
  async openProject(path: string, name?: string) {
    this.publish({ busy: true, error: "" });
    try {
      const opened = await this.openProjectRecord(path);
      const project = {
        ...opened,
        name: name?.trim() || this.snapshot.projects.find((p) => p.id === opened.id)?.name || opened.name || opened.path.split("/").filter(Boolean).at(-1) || opened.path,
      };
      this.publish({
        projects: [
          ...this.snapshot.projects.filter((p) => p.id !== project.id),
          project,
        ],
        busy: false,
      });
      this.createThread(project.id);
      return project;
    } catch (error) {
      this.setError(error);
      return undefined;
    }
  }
  private async resolveChatDirectory(path?: string) {
    const result = await this.invokeExtension<{ path: string }>("workspace", "conversation.directory", path ? { path } : {});
    this.publish({ chatDirectory: result.path });
    return result.path;
  }
  async setChatDirectory(path?: string) {
    const directory = await this.resolveChatDirectory(path?.trim() || undefined);
    this.publish({ customChatDirectory: path?.trim() ? directory : undefined });
    this.persist();
  }
  createThread(projectId?: string) {
    const project = this.snapshot.projects.find((p) => p.id === projectId);
    const thread = newThread(
      project?.id ?? "",
      project?.worktrees[0]?.id ?? this.snapshot.chatDirectory,
      this.snapshot.harnesses.find((h) => h.harness === "codex" && h.available)
        ?.harness ??
        this.snapshot.harnesses.find((h) => h.available)?.harness ??
        "codex",
    );
    this.publish({
      threads: [thread, ...this.snapshot.threads],
      activeId: thread.id,
      review: undefined,
      error: "",
    });
    if (!project && !thread.worktreeId && this.snapshot.connected) {
      const pending = this.resolveChatDirectory(this.snapshot.customChatDirectory).then((path) => {
        this.updateThread(thread.id, { worktreeId: path }, false);
      });
      this.defaultsPending.set(thread.id, pending);
      void pending.catch((error) => this.setError(error));
    }
    if (
      project && thread.worktreeId &&
      this.snapshot.extensions.some((e) => e.id === "resources")
    ) {
      const pending = this.invokeExtension<{
        harness?: string;
        model?: string;
        config?: Record<string, unknown> | null;
        workspace?: string | null;
      }>("resources", "project.get", {
        cwd: project?.path ?? thread.worktreeId,
      }).then(async (defaults) => {
        if (this.configurationEdited.has(thread.id)) return;
        if (
          defaults.workspace &&
          !project?.worktrees.some(
            (worktree) =>
              worktree.path === defaults.workspace ||
              worktree.id === defaults.workspace,
          )
        )
          throw new Error(
            "The saved project workspace is unavailable. Choose an available workspace in project defaults.",
          );
        const current = this.snapshot.threads.find(
          (item) => item.id === thread.id,
        )!;
        this.updateThread(thread.id, {
          harness: defaults.harness ?? thread.harness,
          model: defaults.model ?? thread.model,
          config: mergeConfiguration(
            defaults.config ?? {},
            current.config ?? {},
          ),
          worktreeId: defaults.workspace ?? current.worktreeId,
        });
      });
      this.defaultsPending.set(thread.id, pending);
      void pending.catch((error) => this.setError(error));
    }
  }
  async restoreSessions() {
    const sessions = await this.rpc<SessionSnapshot[]>("session/list");
    for (const session of sessions) {
      if (this.snapshot.threads.some((t) => t.sessionId === session.id))
        continue;
      let project = this.snapshot.projects.find((p) =>
        p.worktrees.some(
          (w) => w.path === session.options.cwd || w.id === session.options.cwd,
        ),
      );
      if (!project && session.options.cwd !== this.snapshot.chatDirectory) {
        try {
          project = await this.openProjectRecord(session.options.cwd);
          this.publish({ projects: [...this.snapshot.projects, project] });
        } catch {
          /* A removed project does not hide its archived conversation. */
        }
      }
      const thread = newThread(
        project?.id ?? "",
        session.options.cwd,
        session.options.harness,
      );
      this.publish({
        threads: [
          ...this.snapshot.threads,
          {
            ...thread,
            sessionId: session.id,
            title: session.metadata?.title ?? "Recovered chat",
            archived: session.metadata?.archived,
            model: session.options.model ?? "",
            config: session.options.config ?? {},
            createdAtMs: session.metadata?.createdAtMs || thread.createdAtMs,
            updatedAtMs: session.metadata?.updatedAtMs || thread.updatedAtMs,
            state: session.state,
            capabilities: session.capabilities,
          },
        ],
      });
    }
  }
  loadMessageTimes(id: string): Promise<void> {
    const thread = this.snapshot.threads.find((item) => item.id === id);
    if (!thread?.sessionId || !this.snapshot.connected) return Promise.resolve();
    const sessionId = thread.sessionId;
    const key = `${id}:${sessionId}`;
    const pending = this.messageTimes.get(key);
    if (pending) return pending;
    const operation = (async () => {
      const times = new Map<string, Date>();
      let afterSequence = 0;
      for (;;) {
        const events = await this.rpc<Event[]>("session/transcript", {
          sessionId, afterSequence, limit: 256,
        });
        if (!events.length) break;
        for (const event of events) {
          if (event.type === "message.user" && typeof event.timestampMs === "number" &&
              Number.isFinite(event.timestampMs) && event.timestampMs > 0 && event.timestampMs <= 8640000000000000)
            times.set(`user-${event.sequence}`, new Date(event.timestampMs));
        }
        const nextSequence = Math.max(...events.map((event) => event.sequence));
        if (events.length < 256 || nextSequence <= afterSequence) break;
        afterSequence = nextSequence;
      }
      const latest = this.snapshot.threads.find((item) => item.id === id);
      if (!latest || latest.sessionId !== sessionId) return;
      let changed = false;
      const messages = latest.messages.map((message) => {
        if (message.role !== "user") return message;
        const date = message.createdAt ? new Date(message.createdAt) : undefined;
        const saved = message.id ? times.get(message.id) : undefined;
        if (saved && (!date || !Number.isFinite(date.getTime()))) {
          changed = true;
          return { ...message, createdAt: saved };
        }
        return message;
      });
      if (changed) this.updateThread(id, { messages }, false);
    })().finally(() => this.messageTimes.delete(key));
    this.messageTimes.set(key, operation);
    return operation;
  }
  async restoreThread(id: string) {
    const thread = this.snapshot.threads.find((t) => t.id === id);
    if (!thread?.sessionId) return;
    const snapshot = await this.rpc<SessionSnapshot>("session/get", {
      sessionId: thread.sessionId,
    });
    let replay = {
      ...thread,
      messages: [],
      tools: [],
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
    } as Thread;
    for (;;) {
      const events = await this.rpc<Event[]>("session/transcript", {
        sessionId: thread.sessionId,
        afterSequence: replay.sequence,
        limit: 256,
      });
      if (!events.length) break;
      replay = reduceEvents(
        await this.bindPromptRecords(replay, events),
        events,
      );
      if (events.length < 256) break;
    }
    let current = snapshot;
    let state = snapshot.state;
    const archived = snapshot.metadata?.archived ?? thread.archived;
    if (!archived && !["closed", "failed"].includes(snapshot.state)) {
      try {
        const resumed = await this.resumeSession(thread);
        current = resumed;
        state = resumed.state;
      } catch (error) {
        if (error instanceof RpcError && error.code === -32001)
          state = "closed";
        else throw error;
      }
    }
    const latest = this.snapshot.threads.find((item) => item.id === id)!;
    const eventFields: (keyof Thread)[] = [
      "messages",
      "tools",
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
    const replayPatch = Object.fromEntries(
      eventFields.map((key) => [key, replay[key]]),
    ) as Partial<Thread>;
    const records = [...(replay.promptRecords ?? [])];
    for (const record of latest.promptRecords ?? [])
      if (!records.some((item) => item.id === record.id)) records.push(record);
    this.updateThread(
      id,
      {
        ...replayPatch,
        promptRecords: records,
        state,
        capabilities: current.capabilities,
        title:
          latest.title !== thread.title
            ? latest.title
            : snapshot.metadata?.title || thread.title,
        archived:
          latest.archived !== thread.archived ? latest.archived : archived,
        turnId: current.turn,
        createdAtMs: snapshot.metadata?.createdAtMs || thread.createdAtMs,
        updatedAtMs:
          latest.updatedAtMs !== thread.updatedAtMs
            ? latest.updatedAtMs
            : snapshot.metadata?.updatedAtMs || thread.updatedAtMs,
      },
      false,
    );
    if (!["closed", "failed"].includes(state) && !archived)
      await this.refreshQueue(id);
  }
  ensureSession(id: string): Promise<Thread> {
    const existing = this.launching.get(id);
    if (existing) return existing;
    const pending = this.ensureSessionInner(id).finally(() =>
      this.launching.delete(id),
    );
    this.launching.set(id, pending);
    return pending;
  }
  private async ensureSessionInner(id: string) {
    const defaults = this.defaultsPending.get(id);
    if (defaults) {
      await defaults;
      this.defaultsPending.delete(id);
    }
    let thread = this.snapshot.threads.find((t) => t.id === id)!;
    if (!thread) throw new Error("Select a conversation first.");
    if (!thread.worktreeId) {
      if (thread.projectId) throw new Error("The project folder is unavailable.");
      const path = await this.resolveChatDirectory(this.snapshot.customChatDirectory);
      this.updateThread(id, { worktreeId: path }, false);
      thread = this.snapshot.threads.find((t) => t.id === id)!;
    }
    if (!thread.sessionId) {
      const options = {
        harness: thread.harness,
        cwd: thread.worktreeId,
        model: thread.model || thread.configurations?.find((option) => option.id === "model")?.value || null,
        config: thread.config ?? {},
      };
      const session = await this.rpc<SessionSnapshot>("session/create", options);
      this.updateThread(id, {
        sessionId: session.id,
        capabilities: session.capabilities,
        state: session.state ?? "idle",
      });
    } else if (["closed", "failed"].includes(thread.state)) {
      if (!thread.capabilities?.resume)
        throw new Error(
          "This harness cannot resume its saved session. Start a new chat to preserve this history.",
        );
      const session = await this.resumeSession(thread);
      this.updateThread(id, {
        state: session.state,
        capabilities: session.capabilities,
        error: undefined,
      });
    }
    thread = this.snapshot.threads.find((t) => t.id === id)!;
    return thread;
  }
  private resumeSession(thread: Thread) {
    const params = { sessionId: thread.sessionId };
    return this.snapshot.extensions.some((e) => e.id === "resources")
      ? this.invokeExtension<SessionSnapshot>(
          "resources",
          "session.resume",
          params,
        )
      : this.rpc<SessionSnapshot>("session/resume", params);
  }
  private async bindPromptRecords(thread: Thread, events: Event[]) {
    const records = (thread.promptRecords ?? []).map((record) => ({
      ...record,
    }));
    for (const event of events) {
      if (event.type !== "message.user" || event.sessionId !== thread.sessionId)
        continue;
      if (records.some((record) => record.nativeSequence === event.sequence))
        continue;
      const content = (event.data as { content?: PromptPart[] }).content;
      if (!Array.isArray(content)) continue;
      const key = await promptKey(content);
      const record = records.find(
        (item) =>
          item.nativeSequence === undefined &&
          item.afterSequence < event.sequence &&
          item.key === key,
      );
      if (record) record.nativeSequence = event.sequence;
    }
    const live =
      this.snapshot.threads.find((item) => item.id === thread.id)
        ?.promptRecords ?? [];
    for (const record of live)
      if (!records.some((item) => item.id === record.id)) records.push(record);
    return { ...thread, promptRecords: records };
  }
  async send(
    id: string,
    text: string,
    attachments: Attachment[] = [],
    delivery: "prompt" | "steer" | "queue" = "prompt",
  ) {
    if (!text.trim() && !attachments.length) return;
    const thread = await this.ensureSession(id);
    const selectedContext = thread.contextItems ?? [];
    const textualContext = selectedContext.filter((item) => !item.skill);
    const skills = selectedContext.flatMap((item) =>
      item.skill ? [item.skill] : [],
    );
    if (
      skills.length &&
      (thread.capabilities?.metadata as Record<string, unknown> | undefined)
        ?.nativeSkillInputs !== true
    )
      throw new Error(
        "This harness does not accept selected native skill paths. Remove the native skill or choose a supported harness.",
      );
    const promptText = textualContext.length
      ? `${text}\n\n${contextText(textualContext)}`
      : text;
    const content: PromptPart[] = [{ type: "text", data: promptText }];
    const retainedAttachments: Attachment[] = [];
    if (attachments.some((attachment) => attachment.binary)) {
      if (!this.snapshot.extensions.some((e) => e.id === "workspace"))
        throw new Error("Enable the workspace extension to attach media or binary files.");
    }
    if (attachments.some((attachment) => attachment.binary && !(attachment.path && attachment.assetId)))
      await this.openWorkspace(thread);
    for (const attachment of attachments) {
      let retained = { ...attachment, data: "" };
      if (attachment.image) {
        if (!thread.capabilities?.images)
          throw new Error(
            "This harness does not accept images. Remove the image or choose an image-capable harness.",
          );
        content.push({
          type: "image",
          data: { data: attachment.data, mime_type: attachment.mime },
        });
      } else if (attachment.binary) {
        const asset =
          attachment.path && attachment.assetId
            ? { path: attachment.path, assetId: attachment.assetId }
            : await this.invokeExtension<{ path: string; assetId: string }>(
                "workspace",
                "assets.write",
                {
                  projectId: thread.worktreeId,
                  name: attachment.name,
                  data: attachment.data,
                },
              );
        content.push({
          type: "text",
          data: `Attached file available on disk: ${asset.path}\nName: ${attachment.name}. Use the harness's native file tools if they support this format.`,
        });
        retained = { ...retained, ...asset };
        this.updateThread(id, {
          attachments: this.snapshot.threads
            .find((t) => t.id === id)
            ?.attachments?.map((a) =>
              a.id === attachment.id
                ? { ...a, path: asset.path, assetId: asset.assetId }
                : a,
            ),
        });
      } else {
        content.push({
          type: "text",
          data: `Attached file: ${attachment.name}\n${attachment.data}`,
        });
      }
      retainedAttachments.push(retained);
    }
    content.push(
      ...skills.map((skill) => ({ type: "skill" as const, data: skill })),
    );
    if (delivery === "steer" && !thread.capabilities?.steering)
      throw new Error(
        "This harness does not support steering. Queue a follow-up instead.",
      );
    if (delivery === "prompt" && !["idle"].includes(thread.state))
      throw new Error(
        "Choose Queue follow-up or Steer while the agent is working.",
      );
    if (delivery === "prompt")
      this.updateThread(id, {
        state: "running",
        error: undefined,
        turnId: undefined,
      });
    try {
      const params =
        attachments.length || skills.length
          ? { sessionId: thread.sessionId, content }
          : { sessionId: thread.sessionId, text: promptText };
      if (
        new TextEncoder().encode(JSON.stringify(params)).length >
        12 * 1024 * 1024
      )
        throw new Error(
          "Keep the complete message, including encoded images, under 12 MB.",
        );
      const record = {
        id: crypto.randomUUID(),
        key: await promptKey(content),
        afterSequence: thread.sequence,
        text,
        attachments: retainedAttachments,
        contextItems: selectedContext,
      };
      const beforeSend = this.snapshot.threads.find((item) => item.id === id)!;
      this.updateThread(id, {
        promptRecords: [...(beforeSend.promptRecords ?? []), record],
      });
      await this.rpc(
        `session/${delivery === "prompt" ? "prompt" : delivery}`,
        params,
      );
      const current = this.snapshot.threads.find((t) => t.id === id)!;
      const sentIds = new Set(attachments.map((a) => a.id));
      const contextIds = new Set(selectedContext.map((item) => item.id));
      this.updateThread(id, {
        submitted: true,
        draft: current.draft === text ? "" : current.draft,
        attachments: (current.attachments ?? []).filter(
          (a) => !sentIds.has(a.id),
        ),
        contextItems: current.contextItems?.filter(
          (item) => !contextIds.has(item.id),
        ),
      });
      if (thread.title === "New chat") {
        const title =
          text.replace(/\s+/g, " ").slice(0, 64) || "Attached files";
        this.updateThread(id, { title });
        void this.rpc("session/metadata", {
          sessionId: thread.sessionId,
          title,
        }).catch((e) => this.setError(e));
      }
      if (delivery === "queue") await this.refreshQueue(id);
    } catch (error) {
      if (delivery === "prompt") this.updateThread(id, { state: "idle" });
      throw error;
    }
  }
  async refreshQueue(id: string) {
    const thread = this.snapshot.threads.find((t) => t.id === id)!;
    const queue = await this.rpc<{ items: Queued[]; paused: boolean }>(
      "session/queue/list",
      { sessionId: thread.sessionId },
    );
    this.updateThread(
      id,
      { queue: queue.items, queuePaused: queue.paused },
      false,
    );
  }
  async editQueue(id: string, itemId: string, text?: string) {
    const thread = this.snapshot.threads.find((t) => t.id === id)!;
    const item = thread.queue?.find((q) => q.id === itemId);
    const preserved = item?.prompt.content.slice(1) ?? [];
    await this.rpc(
      text === undefined ? "session/queue/remove" : "session/queue/update",
      {
        sessionId: thread.sessionId,
        id: itemId,
        ...(text === undefined
          ? {}
          : preserved.length
            ? { content: [{ type: "text", data: text }, ...preserved] }
            : { text }),
      },
    );
    if (item) {
      const key = await promptKey(item.prompt.content);
      const current = this.snapshot.threads.find((t) => t.id === id)!;
      const record = current.promptRecords?.find(
        (entry) => entry.nativeSequence === undefined && entry.key === key,
      );
      if (record) {
        const nextKey =
          text === undefined
            ? undefined
            : await promptKey([{ type: "text", data: text }, ...preserved]);
        this.updateThread(id, {
          promptRecords: current.promptRecords?.flatMap((entry) =>
            entry.id !== record.id
              ? [entry]
              : nextKey
                ? [
                    {
                      ...entry,
                      text: text!,
                      key: nextKey,
                      contextItems: entry.contextItems.filter(
                        (item) => item.skill,
                      ),
                    },
                  ]
                : [],
          ),
        });
      }
    }
    await this.refreshQueue(id);
  }
  async pauseQueue(id: string, paused: boolean) {
    const thread = this.snapshot.threads.find((t) => t.id === id)!;
    await this.rpc("session/queue/pause", {
      sessionId: thread.sessionId,
      paused,
    });
    await this.refreshQueue(id);
  }
  async rename(id: string, title: string) {
    const thread = this.snapshot.threads.find((t) => t.id === id)!;
    if (!title.trim()) return;
    if (thread.sessionId)
      await this.rpc("session/metadata", {
        sessionId: thread.sessionId,
        title: title.trim(),
      });
    this.updateThread(id, { title: title.trim() });
  }
  async archive(id: string, archived: boolean) {
    const thread = this.snapshot.threads.find((t) => t.id === id)!;
    if (thread.sessionId)
      await this.rpc("session/metadata", {
        sessionId: thread.sessionId,
        archived,
      });
    this.updateThread(id, { archived });
  }
  async loadControls(id: string) {
    const thread = await this.ensureSession(id);
    const configuration = await this.rpc<Configuration[]>(
      "agent/configuration",
      { sessionId: thread.sessionId },
    );
    const models = thread.capabilities?.models
      ? await this.rpc<{ id: string; label: string }[]>("agent/models", {
          sessionId: thread.sessionId,
        })
      : [];
    this.updateThread(id, { configurations: configuration, models }, false);
  }
  async previewControls(id: string) {
    const defaults = this.defaultsPending.get(id);
    if (defaults) await defaults;
    const thread = this.snapshot.threads.find((item) => item.id === id);
    if (!thread || !this.snapshot.connected || !thread.worktreeId) return;
    if (thread.sessionId) return this.loadControls(id);
    const options = { harness: thread.harness, cwd: thread.worktreeId, model: thread.model || null, config: thread.config ?? {} };
    const key = `${id}:${JSON.stringify(options)}`;
    const existing = this.controlPreviews.get(key);
    if (existing) return existing;
    const pending = (async () => {
      const result = await this.rpc<{ configuration: Configuration[]; models: { id: string; label: string }[] }>("runtime/configuration", options);
      const current = this.snapshot.threads.find((item) => item.id === id);
      if (!current || current.sessionId || current.harness !== options.harness || current.worktreeId !== options.cwd || (current.model || null) !== options.model || JSON.stringify(current.config ?? {}) !== JSON.stringify(options.config)) return;
      this.updateThread(id, { configurations: result.configuration, models: result.models }, false);
    })().finally(() => this.controlPreviews.delete(key));
    this.controlPreviews.set(key, pending);
    return pending;
  }
  async configure(id: string, key: string, value: unknown) {
    return this.configureMany(id, { [key]: value });
  }
  async configureMany(id: string, config: Record<string, unknown>) {
    const thread = this.snapshot.threads.find((t) => t.id === id)!;
    if (!thread.sessionId) {
      this.updateThread(id, {
        config: { ...thread.config, ...config },
        ...("model" in config ? { model: String(config.model) } : {}),
      });
      return;
    }
    await this.rpc("agent/configure", {
      sessionId: thread.sessionId,
      config,
    });
    this.updateThread(id, {
      config: { ...thread.config, ...config },
      ...("model" in config ? { model: String(config.model) } : {}),
    });
    await this.loadControls(id);
  }
  async compact(id: string) {
    const thread = await this.ensureSession(id);
    await this.rpc("session/compact", { sessionId: thread.sessionId });
  }
  async fork(
    id: string,
    messageId?: string,
    position?: "before_user" | "after_assistant",
  ) {
    const thread = this.snapshot.threads.find((t) => t.id === id)!;
    const snapshot = await this.rpc<SessionSnapshot>("session/fork", {
      sessionId: thread.sessionId,
      ...(messageId ? { messageId, position } : {}),
    });
    const fork = {
      ...newThread(thread.projectId, thread.worktreeId, thread.harness),
      sessionId: snapshot.id,
      capabilities: snapshot.capabilities,
      title: `${thread.title} · fork`,
      state: snapshot.state,
      model: thread.model,
      config: thread.config,
      promptRecords:
        position === "before_user"
          ? []
          : thread.promptRecords?.filter(
              (record) =>
                record.nativeSequence !== undefined &&
                record.nativeSequence <= snapshot.sequence,
            ),
    };
    this.publish({
      threads: [fork, ...this.snapshot.threads],
      activeId: fork.id,
    });
    await this.restoreThread(fork.id);
    return fork.id;
  }
  private async recoveredPrompt(
    input: Thread["messages"][number]["content"],
    record?: NonNullable<Thread["promptRecords"]>[number],
  ) {
    const content =
      typeof input === "string"
        ? [{ type: "text" as const, text: input }]
        : input;
    const draft =
      record?.text ??
      content
        .filter((p) => p.type === "text")
        .map((p) => p.text)
        .join("\n");
    const attachments: Attachment[] = record
      ? await Promise.all(
          record.attachments.map(async (attachment, index) => {
            if (attachment.binary) {
              if (!attachment.assetId)
                throw new Error(
                  `The saved attachment ${attachment.name} has no retained asset. Reattach it before rewinding.`,
                );
              const asset = await this.invokeExtension<{ data: string }>(
                "workspace",
                "assets.read",
                { assetId: attachment.assetId },
              );
              return { ...attachment, data: asset.data };
            }
            const part = content[index + 1];
            if (attachment.image) {
              if (!part || part.type !== "image")
                throw new Error(
                  `The native image for ${attachment.name} is unavailable.`,
                );
              const match = /^data:([^;]+);base64,(.*)$/.exec(part.image);
              if (!match)
                throw new Error(
                  `The native image for ${attachment.name} is unavailable.`,
                );
              return { ...attachment, data: match[2] };
            }
            const prefix = `Attached file: ${attachment.name}\n`;
            if (!part || part.type !== "text" || !part.text.startsWith(prefix))
              throw new Error(
                `The native text for ${attachment.name} is unavailable.`,
              );
            return { ...attachment, data: part.text.slice(prefix.length) };
          }),
        )
      : content
          .filter((p) => p.type === "image")
          .flatMap((p) => {
            const match = /^data:([^;]+);base64,(.*)$/.exec(p.image);
            return match
              ? [
                  {
                    id: crypto.randomUUID(),
                    name: "Restored image",
                    mime: match[1],
                    data: match[2],
                    image: true,
                  },
                ]
              : [];
          });
    return { draft, attachments, contextItems: record?.contextItems ?? [] };
  }
  async editInBranch(
    id: string,
    node: {
      id: string;
      text: string;
      metadata?: { prompt?: { content: PromptPart[] } };
    },
  ) {
    const thread = this.snapshot.threads.find((item) => item.id === id)!;
    const parts = node.metadata?.prompt?.content ?? [
      { type: "text" as const, data: node.text },
    ];
    if (
      !Array.isArray(parts) ||
      parts.some((part) => !["text", "image", "skill"].includes(part.type))
    )
      throw new Error(
        "This native message contains an unavailable resource. Reattach it before creating an edit branch.",
      );
    const nativeText = parts
      .filter((part) => part.type === "text")
      .map((part) => part.data)
      .join("\n");
    const synthetic = reduceEvents(
      { ...newThread("", "", thread.harness), sessionId: "draft" },
      [
        {
          sessionId: "draft",
          sequence: 1,
          type: "message.user",
          data: { content: parts },
        },
      ],
    ).messages[0];
    const key = await promptKey(parts);
    const anchored = thread.promptRecords?.find(
      (item) => `user-${item.nativeSequence}` === node.id,
    );
    const candidates = (thread.promptRecords ?? []).filter((item) => {
      if (item.key === key) return true;
      const message = thread.messages.find(
        (entry) => entry.id === `user-${item.nativeSequence}`,
      );
      const content =
        message &&
        (typeof message.content === "string"
          ? [{ type: "text" as const, text: message.content }]
          : message.content);
      return (
        content
          ?.filter((part) => part.type === "text")
          .map((part) => part.text)
          .join("\n") === nativeText &&
        JSON.stringify(content?.filter((part) => part.type === "image")) ===
          JSON.stringify(
            (Array.isArray(synthetic.content) ? synthetic.content : []).filter(
              (part) => part.type === "image",
            ),
          )
      );
    });
    const original = (record: NonNullable<Thread["promptRecords"]>[number]) =>
      JSON.stringify({
        text: record.text,
        attachments: record.attachments,
        contextItems: record.contextItems,
      });
    if (!anchored && new Set(candidates.map(original)).size > 1)
      throw new Error(
        "Several original prompts match this native entry. Edit the recorded chat message to recover its exact attachments and context.",
      );
    const record = anchored ?? candidates[0];
    const sourceMessage =
      record &&
      thread.messages.find(
        (item) => item.id === `user-${record.nativeSequence}`,
      );
    const recovered = await this.recoveredPrompt(
      sourceMessage?.content ?? synthetic.content,
      record,
    );
    if (!record) {
      recovered.draft = nativeText;
      recovered.contextItems = parts
        .filter((part) => part.type === "skill")
        .map((part) => ({
          id: crypto.randomUUID(),
          kind: "resource",
          label: `Skill: ${part.data.name}`,
          text: `Native skill selected from ${part.data.path}`,
          skill: part.data,
        }));
    }
    const child = await this.fork(id, node.id, "before_user");
    this.updateThread(child, recovered);
    window.dispatchEvent(
      new CustomEvent("mpai:composer-text", { detail: { threadId: child } }),
    );
    return child;
  }
  async editFrom(
    id: string,
    messageId: string,
    scope: "conversation_only" | "conversation_and_files" = "conversation_only",
  ) {
    const thread = this.snapshot.threads.find((t) => t.id === id)!;
    const index =
      thread.turnGroups?.findIndex((g) => g.messageIds.includes(messageId)) ??
      -1;
    const message = thread.messages.find((m) => m.id === messageId);
    if (index < 0 || !message || message.role !== "user")
      throw new Error("Choose a recorded user message to edit.");
    const content =
      typeof message.content === "string"
        ? [{ type: "text" as const, text: message.content }]
        : message.content;
    const record = thread.promptRecords?.find(
      (item) => `user-${item.nativeSequence}` === messageId,
    );
    const recovered = await this.recoveredPrompt(content, record);
    await this.ensureSession(id);
    await this.rpc("session/rollback", {
      sessionId: thread.sessionId,
      turns: thread.turnGroups!.length - index,
      scope,
    });
    await this.restoreThread(id);
    this.updateThread(id, recovered);
    window.dispatchEvent(
      new CustomEvent("mpai:composer-text", { detail: { threadId: id } }),
    );
  }
  async rollback(
    id: string,
    turns: number,
    scope: "conversation_only" | "conversation_and_files" = "conversation_only",
  ) {
    const thread = this.snapshot.threads.find((t) => t.id === id)!;
    await this.rpc("session/rollback", {
      sessionId: thread.sessionId,
      turns,
      scope,
    });
    await this.restoreThread(id);
  }
  invokeExtension<T>(extensionId: string, command: string, args: unknown) {
    return this.rpc<T>("extension/invoke", {
      extensionId,
      command,
      arguments: args,
    });
  }
  async refreshExtensions() {
    this.publish({
      extensions: await this.rpc<ExtensionInfo[]>("extension/list"),
    });
  }
  async cancel(id: string) {
    const thread = this.snapshot.threads.find((t) => t.id === id);
    if (thread?.sessionId)
      await this.rpc("session/cancel", {
        sessionId: thread.sessionId,
        ...(thread.turnId ? { turnId: thread.turnId } : {}),
      });
  }
  async close(id: string) {
    const thread = this.snapshot.threads.find((t) => t.id === id);
    if (thread?.sessionId && thread.state !== "closed")
      await this.rpc("session/close", { sessionId: thread.sessionId });
    if (
      this.snapshot.threads.find((t) => t.id === id)?.sessionId ===
      thread?.sessionId
    )
      this.updateThread(id, { state: "closed", permissions: [] });
  }
  async respond(id: string, option: string, data: unknown) {
    const thread = this.active();
    if (!thread?.sessionId) return;
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
      !thread?.projectId ||
      !this.snapshot.projects.find((p) => p.id === thread.projectId)?.git
    )
      return;
    const review = await this.extension<Review>("review", {
      projectId: thread.projectId,
      worktreeId: thread.worktreeId,
    });
    if (this.snapshot.activeId === thread.id) this.publish({ review });
  }
  async createWorktree(branch: string) {
    const thread = this.active();
    if (!thread?.projectId) throw new Error("Open a project first.");
    if (thread.sessionId)
      throw new Error("Start a new chat before changing its worktree.");
    const tree = await this.extension<
      Project["worktrees"][number] & {
        setup?: { completed: boolean; message?: string };
      }
    >("worktree.create", { projectId: thread.projectId, branch, setup: true });
    const worktrees = await this.extension<Project["worktrees"]>(
      "worktree.list",
      { projectId: thread.projectId },
    );
    this.publish({
      projects: this.snapshot.projects.map((p) =>
        p.id === thread.projectId ? { ...p, worktrees } : p,
      ),
      review: undefined,
    });
    this.updateThread(thread.id, { worktreeId: tree.id });
    if (tree.setup?.completed === false)
      this.setError(
        `Worktree created, but setup failed: ${tree.setup.message ?? "Open Worktree setup to retry."}`,
      );
  }
  async refreshProject(id: string) {
    const project = this.snapshot.projects.find((p) => p.id === id);
    if (!project) return;
    const opened = await this.openProjectRecord(project.path);
    this.publish({
      projects: this.snapshot.projects.map((p) =>
        p.id === id ? { ...p, ...opened, name: p.name || opened.name } : p,
      ),
    });
  }
  async removeWorktree() {
    const thread = this.active();
    if (!thread || thread.sessionId)
      throw new Error("Start a new chat before removing its worktree.");
    await this.extension("worktree.remove", {
      projectId: thread.projectId,
      worktreeId: thread.worktreeId,
    });
    const worktrees = await this.extension<Project["worktrees"]>(
      "worktree.list",
      { projectId: thread.projectId },
    );
    this.publish({
      projects: this.snapshot.projects.map((p) =>
        p.id === thread.projectId ? { ...p, worktrees } : p,
      ),
      review: undefined,
    });
    this.updateThread(thread.id, { worktreeId: worktrees[0]?.id ?? "" });
  }
  async poll() {
    if (this.polling || !this.snapshot.connected) return;
    this.polling = true;
    try {
      for (const thread of this.snapshot.threads.filter(
        (t) =>
          t.sessionId && !["closed", "failed", "starting"].includes(t.state),
      )) {
        try {
          const events = await this.rpc<Event[]>("session/events", {
            sessionId: thread.sessionId,
            afterSequence: thread.sequence,
            limit: 256,
          });
          if (!events.length) continue;
          const current = this.snapshot.threads.find(
            (t) => t.id === thread.id,
          )!;
          if (
            current.sessionId !== thread.sessionId ||
            ["closed", "starting"].includes(current.state)
          )
            continue;
          const bound = await this.bindPromptRecords(current, events);
          const latest = this.snapshot.threads.find(
            (t) => t.id === current.id,
          )!;
          if (latest.sessionId !== current.sessionId) continue;
          const next = reduceEvents(
            { ...latest, promptRecords: bound.promptRecords },
            events,
          );
          if (typeof document !== "undefined")
            void this.notificationService
              .events(next, events, this.snapshot.notifications)
              .catch((error) =>
                this.setError(
                  new Error(`Desktop notification failed: ${String(error)}`),
                ),
              );
          this.publish({
            threads: this.snapshot.threads.map((t) =>
              t.id === next.id ? next : t,
            ),
          });
          if (
            events.some((e) => e.type === "turn.ended") &&
            next.id === this.snapshot.activeId
          )
            await this.review().catch((error) => this.setError(error));
        } catch (error) {
          const current = this.snapshot.threads.find((t) => t.id === thread.id);
          if (
            !current ||
            current.sessionId !== thread.sessionId ||
            ["closed", "starting"].includes(current.state)
          )
            continue;
          this.updateThread(thread.id, {
            state: "failed",
            error: error instanceof Error ? error.message : String(error),
            permissions: [],
          });
        }
      }
    } finally {
      this.polling = false;
    }
  }
}
