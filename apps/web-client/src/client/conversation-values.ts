import type {
  ConversationExecutionContext,
  Execution,
  Message,
} from "@swarmie/plugin-conversations-browser";

function object(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
function text(value: unknown): value is string {
  return typeof value === "string";
}
type ConversationProjection = {
  conversationId: string;
  phase: Execution["phase"];
  label: string;
  messages: Message[];
  admitted: string[];
  omittedMessages: number;
  canAcknowledge: boolean;
  executionContext: ConversationExecutionContext | undefined;
  lifecycleWarning: string | undefined;
};
function executionContext(value: unknown): ConversationExecutionContext | undefined {
  if (value === undefined || value === null) {
    return undefined;
  }
  if (
    !object(value) ||
    value.version !== 1 ||
    !text(value.nodeId) ||
    !text(value.principal) ||
    !text(value.path) ||
    value.path.length === 0 ||
    value.owner !== "git" ||
    !text(value.identity)
  ) {
    throw new Error("The node returned an invalid execution directory.");
  }
  let identity: unknown;
  try {
    identity = JSON.parse(value.identity);
  } catch {
    throw new Error("The node returned an invalid checkout identity.");
  }
  if (
    !Array.isArray(identity) ||
    identity.length !== 3 ||
    !identity.every((part: unknown) => text(part) && part.length > 0) ||
    identity[0] !== value.path
  ) {
    throw new Error("The node returned an invalid checkout identity.");
  }
  return {
    version: 1,
    nodeId: value.nodeId,
    principal: value.principal,
    path: value.path,
    owner: "git",
    identity: value.identity,
  };
}

export function sameExecutionContext(
  left: ConversationExecutionContext | undefined,
  right: ConversationExecutionContext | undefined,
): boolean {
  return (
    left === right ||
    (left !== undefined &&
      right !== undefined &&
      left.version === right.version &&
      left.nodeId === right.nodeId &&
      left.principal === right.principal &&
      left.path === right.path &&
      left.owner === right.owner &&
      left.identity === right.identity)
  );
}

function executionPhase(value: unknown): Execution["phase"] {
  switch (value) {
    case "idle":
    case "queued":
    case "working":
    case "stopping":
    case "stopped":
    case "completed":
    case "failed":
    case "unknown": {
      return value;
    }
    default: {
      throw new Error("The node returned an invalid execution phase.");
    }
  }
}
export function conversationProjection(value: unknown): ConversationProjection {
  if (
    !object(value) ||
    !text(value.conversationId) ||
    !Array.isArray(value.messages) ||
    !text(value.phase) ||
    !text(value.label) ||
    !Array.isArray(value.admitted)
  ) {
    throw new Error("The node returned an invalid conversation projection.");
  }
  const phase = executionPhase(value.phase);
  const messages = value.messages.map<Message>((message: unknown) => {
    if (
      !object(message) ||
      !text(message.id) ||
      !text(message.text) ||
      (message.role !== "user" && message.role !== "assistant" && message.role !== "tool")
    ) {
      throw new Error("The node returned an invalid message.");
    }
    if (message.truncated !== undefined && typeof message.truncated !== "boolean") {
      throw new Error("The node returned invalid message truncation metadata.");
    }
    return {
      id: message.id,
      role: message.role,
      text: message.text,
      ...(message.truncated === undefined ? {} : { truncated: message.truncated }),
    };
  });
  const admitted = value.admitted.map((id: unknown) => {
    if (!text(id)) {
      throw new Error("The node returned an invalid admission receipt.");
    }
    return id;
  });
  const omittedMessages = value.omittedMessages ?? 0;
  if (
    typeof omittedMessages !== "number" ||
    !Number.isSafeInteger(omittedMessages) ||
    omittedMessages < 0
  ) {
    throw new Error("The node returned invalid history truncation metadata.");
  }
  if (value.canAcknowledge !== undefined && typeof value.canAcknowledge !== "boolean") {
    throw new Error("The node returned invalid recovery metadata.");
  }
  return {
    conversationId: value.conversationId,
    phase,
    label: value.label,
    messages,
    admitted,
    omittedMessages,
    canAcknowledge: value.canAcknowledge === true,
    executionContext: executionContext(value.executionContext),
    lifecycleWarning:
      value.lifecycleWarning === undefined || value.lifecycleWarning === null
        ? undefined
        : checkedWarning(value.lifecycleWarning),
  };
}

export function conversationMetadata(value: unknown) {
  if (
    !object(value) ||
    !text(value.id) ||
    !text(value.title) ||
    !text(value.account) ||
    !text(value.model) ||
    !text(value.effort) ||
    typeof value.archived !== "boolean" ||
    typeof value.pinned !== "boolean"
  ) {
    throw new Error("The node returned invalid conversation metadata.");
  }
  return {
    id: value.id,
    title: value.title,
    account: value.account,
    model: value.model,
    effort: value.effort,
    archived: value.archived,
    pinned: value.pinned,
    executionContext: executionContext(value.executionContext),
    lifecycleWarning:
      value.lifecycleWarning === undefined || value.lifecycleWarning === null
        ? undefined
        : checkedWarning(value.lifecycleWarning),
  };
}

export function conversationCatalogPage(value: unknown) {
  if (
    !object(value) ||
    !Array.isArray(value.entries) ||
    value.entries.length > 128 ||
    !(
      value.nextCursor === null ||
      (typeof value.nextCursor === "string" &&
        value.nextCursor.length > 0 &&
        value.nextCursor.length <= 512)
    )
  ) {
    throw new Error("The node conversation index is invalid.");
  }
  return { entries: value.entries.map(conversationMetadata), nextCursor: value.nextCursor };
}

/** Only a typed refusal confirms that no submission was admitted. */
export function refusalReason(value: unknown): string | undefined {
  return object(value) && value.status === "refused" && text(value.reason)
    ? value.reason
    : undefined;
}

function checkedWarning(value: unknown): string {
  if (!text(value) || value.length > 4096) {
    throw new Error("The node returned invalid retention metadata.");
  }
  return value;
}
