import type { ThreadMessageLike } from "@assistant-ui/react";
import type {
  Capabilities,
  SettingDescriptor,
  ModelDescriptor,
  SubmissionReceipt,
  ExecutionPolicy,
  AgentTask,
  PromptContent,
  RuntimeEvent,
  SessionSnapshot as RuntimeSessionSnapshot,
  UiManifest,
} from "@swarmie/contracts";
import { decodeContract } from "@swarmie/contracts/validation";

import type { ContextItem, FileLocation } from "~/src/context";

export type {
  ConfigurationReceipt,
  ConfigurationPreview,
  SubmissionReceipt,
  AgentTask,
} from "@swarmie/contracts";

export type Worktree = {
  id: string;
  path: string;
  branch: string | null;
  isMain: boolean;
  locked: boolean;
  prunable: boolean;
};
export type Project = {
  id: string;
  revision?: number;
  gitId?: string;
  path: string;
  name: string;
  git?: boolean;
  worktrees: Worktree[];
};
export type Permission = {
  id: string;
  title: string;
  description?: string;
  options: { id: string; label: string }[];
  kind?: string;
  metadata?: { questions?: Question[]; [key: string]: unknown };
};
export type Question = {
  id: string;
  prompt: string;
  options: { id: string; label: string }[];
  multiple?: boolean;
  allowText?: boolean;
  secret?: boolean;
};
export type Capability = Partial<Capabilities>;
export type Configuration = SettingDescriptor;
export type Model = ModelDescriptor;
export type Attachment = {
  id: string;
  name: string;
  mime: string;
  data: string;
  image: boolean;
  binary?: boolean;
  path?: string;
  assetId?: string;
};
export type PromptPart =
  | { type: "text"; data: string }
  | { type: "image"; data: { mime_type: string; data: string } }
  | { type: "skill"; data: { name: string; path: string } };
export type Queued = {
  id: string;
  prompt: { content: PromptContent[] };
  createdAtMs: number;
};
export type ExtensionView = {
  id: string;
  title: string;
  kind: string;
  command: string;
};
export type ExtensionInfo = {
  id: string;
  ui?: UiManifest | null;
  views?: ExtensionView[];
  commands?: { name: string; title: string; mutates: boolean }[];
};
export type SessionSnapshot = RuntimeSessionSnapshot;

export type Tool = {
  id: string;
  name: string;
  status: string;
  input?: unknown;
  output?: unknown;
  metadata?: Record<string, unknown>;
};
export type Event = {
  timestampMs?: number;
  sessionId: string;
  sequence: number;
  type: string;
  turnId?: string;
  data: unknown;
  raw?: Record<string, unknown>;
};
export type Thread = {
  projectRevision?: number;
  projectDirectory?: string;
  submitted?: boolean;
  id: string;
  title: string;
  projectId: string;
  worktreeId: string;
  harness: string;
  model: string;
  actualModel?: string;
  sessionId?: string;
  state: string;
  sequence: number;
  messages: ThreadMessageLike[];
  tools: Tool[];
  tasks?: AgentTask[];
  permissions: Permission[];
  pinned: boolean;
  error?: string;
  assistantId?: string;
  itemId?: string;
  messageAnchors?: Record<string, string>;
  outputFiles?: string[];
  fileDrafts?: Record<string, { content: string; original: string }>;
  submissionKeys?: {
    key: string;
    recordId: string;
    receipt?: SubmissionReceipt;
  }[];
  promptRecords?: {
    id: string;
    key: string;
    afterSequence: number;
    nativeSequence?: number;
    text: string;
    attachments: Attachment[];
    contextItems: ContextItem[];
  }[];
  capabilities?: Capability;
  configurations?: Configuration[];
  executionPolicy?: ExecutionPolicy;
  models?: Model[];
  config?: Record<string, unknown>;
  archived?: boolean;
  draft?: string;
  attachments?: Attachment[];
  queue?: Queued[];
  queuePaused?: boolean;
  turnId?: string;
  turnGroups?: { turnId: string; messageIds: string[]; toolIds: string[] }[];
  reasoning?: string;
  plan?: unknown;
  subagents?: Record<string, unknown>[];
  usage?: Record<string, unknown>;
  reviewedFiles?: Record<string, string>;
  contextItems?: ContextItem[];
  linkedPr?: { number?: number; url: string };
  comparisonId?: string;
  commandsRevision?: number;
  createdAtMs?: number;
  updatedAtMs?: number;
  snoozeUntil?: number;
  settled?: boolean;
  layout?: {
    panel?: string | null;
    width?: number;
    selectedView?: string;
    openViews?: string[];
    files?: FileLocation[];
    activeFile?: FileLocation;
  };
  context?: {
    usedTokens?: number;
    contextWindowTokens?: number;
    remainingTokens?: number;
    percent?: number;
  };
};
export type Review = {
  diff: string;
  files: { path: string; status: string }[];
  truncated: boolean;
  filesTruncated: boolean;
};
export type Detection = {
  harness: string;
  available: boolean;
  capabilities?: Capability;
  executable?: string | null;
  version?: string | null;
  authentication?: unknown;
  error?: string;
  authState?: string;
  message?: string | null;
};

export function errorMessage(value: unknown, harness: string): string {
  if (
    typeof value === "string" &&
    value.trim().length > 0 &&
    value !== "null" &&
    value !== "undefined"
  ) {
    return value;
  }
  return `${harness} failed without an error message. Check its authentication and provider configuration.`;
}

export function hasConversation(thread: Thread): boolean {
  return (
    thread.submitted === true ||
    thread.submissionKeys?.some(
      (entry) => !entry.receipt || entry.receipt.delivery !== "rejected",
    ) === true ||
    thread.messages.some((message) => message.role === "user")
  );
}

export function newThread(projectId: string, worktreeId: string, harness: string): Thread {
  return {
    id: crypto.randomUUID(),
    title: "New chat",
    projectId,
    worktreeId,
    harness,
    model: "",
    state: "idle",
    sequence: 0,
    messages: [],
    tools: [],
    permissions: [],
    pinned: false,
    createdAtMs: Date.now(),
    updatedAtMs: Date.now(),
  };
}

export function nativeEvents(events: readonly RuntimeEvent[]): Event[] {
  return events.map((event) => ({
    timestampMs: event.timestampMs,
    sessionId: event.sessionId,
    sequence: event.sequence,
    type: event.type,
    data: "data" in event ? event.data : undefined,
    ...(typeof event.turnId === "string" ? { turnId: event.turnId } : {}),
    ...(isRecord(event.raw) ? { raw: event.raw } : {}),
  }));
}

export function isRecord<T>(value: T): value is T & Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
function displayValue(value: unknown): string {
  if (value === undefined || value === null) {
    return "";
  }
  if (typeof value === "string") {
    return value;
  }
  if (typeof value === "number" || typeof value === "boolean" || typeof value === "bigint") {
    return String(value);
  }
  return JSON.stringify(value) ?? "";
}
function messageText(content: ThreadMessageLike["content"] | undefined): string {
  if (typeof content === "string") {
    return content;
  }
  const first = content?.[0];
  return first?.type === "text" ? first.text : "";
}
function taskData(value: unknown): AgentTask {
  return decodeContract("AgentTask", value);
}
function executionPolicyData(value: unknown): ExecutionPolicy {
  return decodeContract("ExecutionPolicy", value);
}
function configurationData(value: unknown): Configuration {
  return decodeContract("SettingDescriptor", value);
}
type NativeToolPatch = Omit<Tool, "name"> & { name?: string };
function toolData(value: unknown): NativeToolPatch {
  if (!isRecord(value) || typeof value.id !== "string" || typeof value.status !== "string") {
    throw new TypeError("Invalid native tool event.");
  }
  return {
    id: value.id,
    status: value.status,
    ...(typeof value.name === "string" ? { name: value.name } : {}),
    input: value.input,
    output: value.output,
    ...(isRecord(value.metadata) ? { metadata: value.metadata } : {}),
  };
}
function permissionOptions(value: unknown): Permission["options"] {
  if (!Array.isArray(value)) {
    throw new TypeError("Invalid native permission options.");
  }
  return value.map((option: unknown) => {
    if (!isRecord(option) || typeof option.id !== "string" || typeof option.label !== "string") {
      throw new TypeError("Invalid native permission option.");
    }
    return { id: option.id, label: option.label };
  });
}
function questionData(value: unknown): Question {
  if (!isRecord(value) || typeof value.id !== "string" || typeof value.prompt !== "string") {
    throw new TypeError("Invalid native question.");
  }
  return {
    id: value.id,
    prompt: value.prompt,
    options: permissionOptions(value.options),
    ...(typeof value.multiple === "boolean" ? { multiple: value.multiple } : {}),
    ...(typeof value.allowText === "boolean" ? { allowText: value.allowText } : {}),
    ...(typeof value.secret === "boolean" ? { secret: value.secret } : {}),
  };
}
function permissionData(value: unknown): Permission {
  if (!isRecord(value) || typeof value.id !== "string" || typeof value.title !== "string") {
    throw new TypeError("Invalid native permission event.");
  }
  const metadata = isRecord(value.metadata) ? value.metadata : undefined;
  return {
    id: value.id,
    title: value.title,
    options: permissionOptions(value.options),
    ...(typeof value.description === "string" ? { description: value.description } : {}),
    ...(typeof value.kind === "string" ? { kind: value.kind } : {}),
    ...(metadata
      ? {
          metadata: {
            ...metadata,
            questions: Array.isArray(metadata.questions)
              ? metadata.questions.map(questionData)
              : undefined,
          },
        }
      : {}),
  };
}
export function promptParts(value: unknown): PromptPart[] {
  if (!Array.isArray(value)) {
    throw new TypeError("Invalid prompt content.");
  }
  return value.map((part: unknown): PromptPart => {
    if (!isRecord(part)) {
      throw new TypeError("Invalid prompt part.");
    }
    if (part.type === "text" && typeof part.data === "string") {
      return { type: "text", data: part.data };
    }
    if (
      part.type === "image" &&
      isRecord(part.data) &&
      typeof part.data.mime_type === "string" &&
      typeof part.data.data === "string"
    ) {
      return { type: "image", data: { mime_type: part.data.mime_type, data: part.data.data } };
    }
    if (
      part.type === "skill" &&
      isRecord(part.data) &&
      typeof part.data.name === "string" &&
      typeof part.data.path === "string"
    ) {
      return { type: "skill", data: { name: part.data.name, path: part.data.path } };
    }
    throw new TypeError("Unsupported prompt content.");
  });
}
function queuedData(value: unknown): Queued {
  const queue = decodeContract("QueuedPrompt", value);
  return { id: queue.id, createdAtMs: queue.createdAtMs, prompt: queue.prompt };
}
function userContent(value: unknown): Exclude<ThreadMessageLike["content"], string> {
  if (!Array.isArray(value)) {
    return [];
  }
  return value.map((part: unknown) => {
    if (!isRecord(part)) {
      throw new TypeError("Invalid native message part.");
    }
    if (
      part.type === "image" &&
      isRecord(part.data) &&
      typeof part.data.mime_type === "string" &&
      typeof part.data.data === "string"
    ) {
      return {
        type: "image" as const,
        image: `data:${part.data.mime_type};base64,${part.data.data}`,
      };
    }
    if (part.type === "resource" && isRecord(part.data) && typeof part.data.uri === "string") {
      return { type: "text" as const, text: `Attached resource: ${part.data.uri}` };
    }
    if (part.type === "skill" && isRecord(part.data) && typeof part.data.name === "string") {
      return { type: "text" as const, text: `Native skill: ${part.data.name}` };
    }
    return { type: "text" as const, text: displayValue(part.data) };
  });
}

type EventThread = Thread & Required<Pick<Thread, "tasks" | "turnGroups" | "messageAnchors">>;

type EventContext = { thread: EventThread; event: Event; data: Record<string, unknown> };
type EventHandler = (context: EventContext) => void;
function applyToolEvent({ thread, event }: EventContext): void {
  const tool = toolData(event.data);
  const group = thread.turnGroups.find((g) => g.turnId === event.turnId);
  if (group && !group.toolIds.includes(tool.id)) {
    group.toolIds.push(tool.id);
  }
  const index = thread.tools.findIndex((t) => t.id === tool.id);
  const existingTool = index === -1 ? undefined : thread.tools[index];
  const metadata = {
    ...existingTool?.metadata,
    ...Object.fromEntries(
      Object.entries(tool.metadata ?? {}).filter(
        ([, value]) => value !== null && value !== undefined,
      ),
    ),
  };
  const merged: Tool = {
    ...existingTool,
    ...tool,
    name: tool.name ?? existingTool?.name ?? "Tool",
    input: tool.input ?? existingTool?.input,
    output: tool.output ?? existingTool?.output,
    metadata,
  };
  if (event.type === "tool.updated" && typeof tool.metadata?.outputDelta === "string") {
    merged.output =
      (typeof existingTool?.output === "string" ? existingTool.output : "") +
      tool.metadata.outputDelta;
  }
  if (index === -1) {
    thread.tools.push(merged);
  } else {
    thread.tools[index] = merged;
  }
}
const eventHandlers = new Map<string, EventHandler>([
  [
    "task.updated",
    ({ thread, event }) => {
      const task = taskData(event.data);
      const { tasks } = thread;
      const existing = tasks.findIndex((item) => item.id === task.id);
      if (existing === -1) {
        tasks.push(task);
      } else {
        tasks[existing] = task;
      }
      if (tasks.length > 256) {
        tasks.splice(0, tasks.length - 256);
      }
    },
  ],
  [
    "message.user",
    ({ thread, event, data }) => {
      const content = userContent(data.content);
      const userId = `user-${event.sequence}`;
      const group = thread.turnGroups.find((g) => g.turnId === event.turnId);
      if (group) {
        group.messageIds.push(userId);
      } else {
        thread.turnGroups.push({
          turnId: event.turnId ?? `turn-${event.sequence}`,
          messageIds: [userId],
          toolIds: [],
        });
      }
      thread.messages.push({
        id: userId,
        role: "user",
        ...(typeof event.timestampMs === "number" &&
        Number.isFinite(event.timestampMs) &&
        event.timestampMs > 0 &&
        event.timestampMs <= 8_640_000_000_000_000
          ? { createdAt: new Date(event.timestampMs) }
          : {}),
        content,
      });
      thread.assistantId = undefined;
      thread.itemId = undefined;
    },
  ],
  [
    "session.updated",
    ({ thread, data }) => {
      const options = isRecord(data.options) ? data.options : undefined;
      if (options) {
        thread.config = isRecord(options.config) ? options.config : {};
        thread.model = typeof options.model === "string" ? options.model : "";
      }
      if (data.executionPolicy !== undefined) {
        thread.executionPolicy = executionPolicyData(data.executionPolicy);
      }
      if (Array.isArray(data.configuration)) {
        thread.configurations = data.configuration.map(configurationData);
      }
    },
  ],
  [
    "text.delta",
    ({ thread, event, data }) => {
      const id = thread.assistantId ?? `assistant-${event.turnId ?? event.sequence}`;
      thread.assistantId = id;
      if (typeof data.item_id === "string" && data.item_id.length > 0) {
        thread.messageAnchors[id] = data.item_id;
      }
      const group = thread.turnGroups.find((g) => g.turnId === event.turnId);
      if (group && !group.messageIds.includes(id)) {
        group.messageIds.push(id);
      }
      const index = thread.messages.findIndex((m) => m.id === id);
      const old = index === -1 ? "" : messageText(thread.messages[index]?.content);
      const message: ThreadMessageLike = {
        id,
        role: "assistant",
        content: [
          {
            type: "text",
            text:
              old +
              (old.length > 0 &&
              thread.itemId !== undefined &&
              typeof data.item_id === "string" &&
              data.item_id.length > 0 &&
              thread.itemId !== data.item_id
                ? "\n\n"
                : "") +
              displayValue(data.text),
          },
        ],
      };
      if (index === -1) {
        thread.messages.push(message);
      } else {
        thread.messages[index] = message;
      }
      thread.itemId =
        typeof data.item_id === "string" && data.item_id.length > 0 ? data.item_id : thread.itemId;
    },
  ],
  ["tool.started", applyToolEvent],
  ["tool.updated", applyToolEvent],
  ["tool.completed", applyToolEvent],
  [
    "file.changed",
    ({ thread, data }) => {
      const paths = [
        data.path,
        data.filePath,
        data.file_path,
        ...(Array.isArray(data.changes)
          ? data.changes.map((change: unknown) => (isRecord(change) ? change.path : undefined))
          : []),
      ].filter((path): path is string => typeof path === "string" && path.length > 0);
      thread.outputFiles = [...new Set([...(thread.outputFiles ?? []), ...paths])];
    },
  ],
  [
    "permission.requested",
    ({ thread, event, data }) => {
      if (!thread.permissions.some((p) => p.id === data.id)) {
        thread.permissions.push(permissionData(event.data));
      }
    },
  ],
  [
    "permission.resolved",
    ({ thread, data }) => {
      thread.permissions = thread.permissions.filter((p) => p.id !== data.id);
    },
  ],
  [
    "state.changed",
    ({ thread, event }) => {
      thread.state = displayValue(event.data);
    },
  ],
  [
    "turn.started",
    ({ thread, event }) => {
      thread.state = "running";
      thread.error = undefined;
      thread.turnId = event.turnId;
      thread.actualModel = undefined;
    },
  ],
  [
    "turn.ended",
    ({ thread }) => {
      thread.state = "idle";
      thread.turnId = undefined;
      thread.permissions = [];
      thread.assistantId = undefined;
      thread.itemId = undefined;
    },
  ],
  [
    "session.ended",
    ({ thread }) => {
      thread.state = "closed";
      thread.permissions = [];
    },
  ],
  [
    "conversation.rolled_back",
    ({ thread, data }) => {
      const removed = thread.turnGroups.splice(-Number(data.turns ?? 0));
      const ids = new Set(removed.flatMap((g) => g.messageIds));
      const tools = new Set(removed.flatMap((g) => g.toolIds));
      thread.messages = thread.messages.filter((m) => !ids.has(m.id ?? ""));
      thread.tools = thread.tools.filter((t) => !tools.has(t.id));
      thread.assistantId = undefined;
      thread.itemId = undefined;
      thread.plan = undefined;
      thread.reasoning = undefined;
      thread.subagents = [];
    },
  ],
  [
    "queue.changed",
    ({ thread, data }) => {
      thread.queue = Array.isArray(data.items) ? data.items.map(queuedData) : [];
      thread.queuePaused = data.paused === true;
    },
  ],
  [
    "reasoning.delta",
    ({ thread, event, data }) => {
      thread.reasoning = (thread.reasoning ?? "") + displayValue(data.text ?? event.data);
    },
  ],
  [
    "plan.updated",
    ({ thread, event }) => {
      thread.plan = event.data;
    },
  ],
  [
    "subagent.updated",
    ({ thread, data }) => {
      const child = data;
      const id = displayValue(
        child.id ?? child.agentId ?? child.sessionId ?? child.parent_tool_use_id ?? "unknown",
      );
      const children = [...(thread.subagents ?? [])];
      const index = children.findIndex((c) => c._id === id);
      if (index === -1) {
        children.push({ ...child, _id: id });
      } else {
        children[index] = { ...children[index], ...child };
      }
      thread.subagents = children;
    },
  ],
  [
    "usage.updated",
    ({ thread, data }) => {
      thread.usage = { ...thread.usage, ...data };
    },
  ],
  [
    "context.updated",
    ({ thread, data }) => {
      thread.context = data;
    },
  ],
  [
    "commands.updated",
    ({ thread }) => {
      thread.commandsRevision = (thread.commandsRevision ?? 0) + 1;
    },
  ],
  [
    "model.rerouted",
    ({ thread, data }) => {
      if (typeof data.toModel === "string") {
        thread.actualModel = data.toModel;
      }
    },
  ],
  [
    "error",
    ({ thread, event, data }) => {
      thread.error = errorMessage(
        data.message === "null" || data.message === null || data.message === undefined
          ? event.raw?.result
          : data.message,
        thread.harness,
      );
    },
  ],
]);

export function reduceEvents(previous: Thread, events: Event[]): Thread {
  const thread: EventThread = {
    ...previous,
    messages: [...previous.messages],
    tools: [...previous.tools],
    tasks: [...(previous.tasks ?? [])],
    permissions: [...previous.permissions],
    messageAnchors: { ...previous.messageAnchors },
    outputFiles: [...(previous.outputFiles ?? [])],
    turnGroups: (previous.turnGroups ?? []).map((group) => ({
      ...group,
      messageIds: [...group.messageIds],
      toolIds: [...group.toolIds],
    })),
  };
  for (const event of events) {
    if (event.sessionId !== thread.sessionId || event.sequence <= thread.sequence) {
      continue;
    }
    thread.sequence = event.sequence;
    const data = isRecord(event.data) ? event.data : {};
    eventHandlers.get(event.type)?.({ thread, event, data });
  }
  return thread;
}

export async function copyText(text: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(text);
  } catch (error) {
    const focused = document.activeElement;
    const input = document.createElement("textarea");
    input.value = text;
    input.setAttribute("aria-hidden", "true");
    input.style.position = "fixed";
    input.style.top = "-10000px";
    document.body.append(input);
    input.select();
    try {
      // Older supported WebViews may reject Async Clipboard; the temporary input is removed and focus restored below.
      // oxlint-disable-next-line typescript/no-deprecated -- This capability fallback preserves copy where Async Clipboard is unavailable.
      if (!document.execCommand("copy")) {
        throw error;
      }
    } finally {
      input.remove();
      if (focused instanceof HTMLElement) {
        focused.focus({ preventScroll: true });
      }
    }
  }
}
