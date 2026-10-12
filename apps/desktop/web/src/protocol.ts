import type {
  Capabilities,
  SettingDescriptor,
  ModelDescriptor,
  ConfigurationReceipt,
  ConfigurationPreview,
  SubmissionReceipt,
  WorkingContext,
  ExecutionPolicy,
} from "../../../../packages/clients/typescript/src/generated";
export type { ConfigurationReceipt, ConfigurationPreview, SubmissionReceipt };
import type { ThreadMessageLike } from "@assistant-ui/react";
import type { ContextItem, FileLocation } from "./context";

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
  prompt: { content: PromptPart[] };
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
  views?: ExtensionView[];
  commands?: { name: string; title: string; mutates: boolean }[];
};
export type SessionSnapshot = {
  id: string;
  workingContext?: WorkingContext | null;
  executionPolicy?: ExecutionPolicy;
  options: {
    harness: string;
    cwd: string;
    projectId?: string | null;
    projectRevision?: number | null;
    model?: string;
    config?: Record<string, unknown>;
  };
  capabilities: Capability;
  state: string;
  sequence: number;
  turn?: string;
  metadata?: {
    title?: string;
    archived?: boolean;
    createdAtMs?: number;
    updatedAtMs?: number;
  };
};
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
  executable?: string;
  version?: string;
  authentication?: unknown;
  error?: string;
  authState?: string;
  message?: string;
};

export function errorMessage(value: unknown, harness: string): string {
  if (
    typeof value === "string" &&
    value.trim() &&
    value !== "null" &&
    value !== "undefined"
  )
    return value;
  return `${harness} failed without an error message. Check its authentication and provider configuration.`;
}

export function hasConversation(thread: Thread): boolean {
  return (
    !!thread.submitted ||
    !!thread.submissionKeys?.some(
      (entry) => !entry.receipt || entry.receipt.delivery !== "rejected",
    ) ||
    thread.messages.some((message) => message.role === "user")
  );
}

export function newThread(
  projectId: string,
  worktreeId: string,
  harness: string,
): Thread {
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

export function reduceEvents(previous: Thread, events: Event[]): Thread {
  const thread: Thread = {
    ...previous,
    messages: [...previous.messages],
    tools: [...previous.tools],
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
    if (
      event.sessionId !== thread.sessionId ||
      event.sequence <= thread.sequence
    )
      continue;
    thread.sequence = event.sequence;
    const data = event.data as Record<string, unknown>;
    switch (event.type) {
      case "message.user": {
        const content = (
          (data.content as { type: string; data: unknown }[]) ?? []
        ).map((part) => {
          if (part.type === "image") {
            const image = part.data as { mime_type: string; data: string };
            return {
              type: "image" as const,
              image: `data:${image.mime_type};base64,${image.data}`,
            };
          }
          if (part.type === "resource")
            return {
              type: "text" as const,
              text: `Attached resource: ${(part.data as { uri: string }).uri}`,
            };
          if (part.type === "skill")
            return {
              type: "text" as const,
              text: `Native skill: ${(part.data as { name: string }).name}`,
            };
          return { type: "text" as const, text: String(part.data ?? "") };
        });
        const userId = `user-${event.sequence}`;
        const group = thread.turnGroups!.find((g) => g.turnId === event.turnId);
        if (group) group.messageIds.push(userId);
        else
          thread.turnGroups!.push({
            turnId: event.turnId ?? `turn-${event.sequence}`,
            messageIds: [userId],
            toolIds: [],
          });
        thread.messages.push({
          id: userId,
          role: "user",
          ...(typeof event.timestampMs === "number" &&
          Number.isFinite(event.timestampMs) &&
          event.timestampMs > 0 &&
          event.timestampMs <= 8640000000000000
            ? { createdAt: new Date(event.timestampMs) }
            : {}),
          content,
        });
        thread.assistantId = undefined;
        thread.itemId = undefined;
        break;
      }
      case "session.updated": {
        const options = data.options as SessionSnapshot["options"] | undefined;
        if (options) {
          thread.config = options.config ?? {};
          thread.model = options.model ?? "";
        }
        if (data.executionPolicy) thread.executionPolicy = data.executionPolicy as ExecutionPolicy;
        if (Array.isArray(data.configuration))
          thread.configurations = data.configuration as Configuration[];
        break;
      }
      case "text.delta": {
        const id =
          thread.assistantId ?? `assistant-${event.turnId ?? event.sequence}`;
        thread.assistantId = id;
        if (data.item_id) thread.messageAnchors![id] = String(data.item_id);
        const group = thread.turnGroups!.find((g) => g.turnId === event.turnId);
        if (group && !group.messageIds.includes(id)) group.messageIds.push(id);
        const index = thread.messages.findIndex((m) => m.id === id);
        const old =
          index < 0
            ? ""
            : (thread.messages[index].content as readonly { text: string }[])[0]
                .text;
        const message: ThreadMessageLike = {
          id,
          role: "assistant",
          content: [
            {
              type: "text",
              text:
                old +
                (old &&
                thread.itemId &&
                data.item_id &&
                thread.itemId !== data.item_id
                  ? "\n\n"
                  : "") +
                String(data.text ?? ""),
            },
          ],
        };
        if (index < 0) thread.messages.push(message);
        else thread.messages[index] = message;
        thread.itemId = data.item_id ? String(data.item_id) : thread.itemId;
        break;
      }
      case "tool.started":
      case "tool.updated":
      case "tool.completed": {
        const tool = event.data as Tool;
        const group = thread.turnGroups!.find((g) => g.turnId === event.turnId);
        if (group && !group.toolIds.includes(tool.id))
          group.toolIds.push(tool.id);
        const index = thread.tools.findIndex((t) => t.id === tool.id);
        const previous = index < 0 ? undefined : thread.tools[index];
        const metadata = {
          ...previous?.metadata,
          ...Object.fromEntries(
            Object.entries(tool.metadata ?? {}).filter(
              ([, value]) => value != null,
            ),
          ),
        };
        const merged: Tool = {
          ...previous,
          ...tool,
          name: tool.name ?? previous?.name ?? "Tool",
          input: tool.input ?? previous?.input,
          output: tool.output ?? previous?.output,
          metadata,
        };
        if (
          event.type === "tool.updated" &&
          typeof tool.metadata?.outputDelta === "string"
        ) {
          merged.output =
            (typeof previous?.output === "string" ? previous.output : "") +
            tool.metadata.outputDelta;
        }
        if (index < 0) thread.tools.push(merged);
        else thread.tools[index] = merged;
        break;
      }
      case "file.changed": {
        const paths = [
          data.path,
          data.filePath,
          data.file_path,
          ...(Array.isArray(data.changes)
            ? data.changes.map((c: Record<string, unknown>) => c.path)
            : []),
        ].filter((path): path is string => typeof path === "string" && !!path);
        thread.outputFiles = [
          ...new Set([...(thread.outputFiles ?? []), ...paths]),
        ];
        break;
      }
      case "permission.requested":
        if (!thread.permissions.some((p) => p.id === data.id))
          thread.permissions.push(event.data as Permission);
        break;
      case "permission.resolved":
        thread.permissions = thread.permissions.filter((p) => p.id !== data.id);
        break;
      case "state.changed":
        thread.state = String(event.data);
        break;
      case "turn.started":
        thread.state = "running";
        thread.error = undefined;
        thread.turnId = event.turnId;
        thread.actualModel = undefined;
        break;
      case "turn.ended":
        thread.state = "idle";
        thread.turnId = undefined;
        thread.permissions = [];
        thread.assistantId = undefined;
        thread.itemId = undefined;
        break;
      case "session.ended":
        thread.state = "closed";
        thread.permissions = [];
        break;
      case "conversation.rolled_back": {
        const removed = thread.turnGroups!.splice(-Number(data.turns ?? 0));
        const ids = new Set(removed.flatMap((g) => g.messageIds));
        const tools = new Set(removed.flatMap((g) => g.toolIds));
        thread.messages = thread.messages.filter((m) => !ids.has(m.id ?? ""));
        thread.tools = thread.tools.filter((t) => !tools.has(t.id));
        thread.assistantId = undefined;
        thread.itemId = undefined;
        thread.plan = undefined;
        thread.reasoning = undefined;
        thread.subagents = [];
        break;
      }
      case "queue.changed":
        thread.queue = (data.items as Queued[]) ?? [];
        thread.queuePaused = !!data.paused;
        break;
      case "reasoning.delta":
        thread.reasoning =
          (thread.reasoning ?? "") + String(data.text ?? event.data ?? "");
        break;
      case "plan.updated":
        thread.plan = event.data;
        break;
      case "subagent.updated": {
        const child = data;
        const id = String(
          child.id ??
            child.agentId ??
            child.sessionId ??
            child.parent_tool_use_id ??
            "unknown",
        );
        const children = [...(thread.subagents ?? [])];
        const index = children.findIndex((c) => c._id === id);
        if (index < 0) children.push({ ...child, _id: id });
        else children[index] = { ...children[index], ...child };
        thread.subagents = children;
        break;
      }
      case "usage.updated":
        thread.usage = { ...thread.usage, ...data };
        break;
      case "context.updated":
        thread.context = data;
        break;
      case "commands.updated":
        thread.commandsRevision = (thread.commandsRevision ?? 0) + 1;
        break;
      case "model.rerouted":
        if (typeof data.toModel === "string") thread.actualModel = data.toModel;
        break;
      case "error":
        thread.error = errorMessage(
          data.message === "null" || data.message == null
            ? event.raw?.result
            : data.message,
          thread.harness,
        );
        break;
    }
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
    document.body.appendChild(input);
    input.select();
    try {
      if (!document.execCommand("copy")) throw error;
    } finally {
      input.remove();
      if (focused instanceof HTMLElement)
        focused.focus({ preventScroll: true });
    }
  }
}
