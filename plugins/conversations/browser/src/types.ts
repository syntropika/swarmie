export type Draft = {
  text: string;
  attachments: string[];
  model: string;
  account: string | null;
  effort: string;
  node: string;
};
export type MessageValue =
  | null
  | boolean
  | number
  | string
  | readonly MessageValue[]
  | { readonly [key: string]: MessageValue };
export type Message = {
  id: string;
  role: "user" | "assistant" | "tool";
  text: string;
  /** Namespaced presentation kind and public payload for a plugin renderer. */
  kind?: string;
  data?: MessageValue;
  interrupted?: boolean;
  /** The node shortened this presentation text. Canonical history is unchanged. */
  truncated?: boolean;
  tool?: "working" | "completed";
};
export type Input = { id: string; text: string; mode: "queue" | "steer" };
export type Execution = {
  phase:
    | "idle"
    | "queued"
    | "working"
    | "stopping"
    | "stopped"
    | "completed"
    | "failed"
    | "unknown";
  label: string;
  queue: Input[];
  steering: Input[];
  /** Earlier messages omitted from the bounded node projection. */
  omittedMessages?: number;
  /** The node can close unfinished tool records after explicit acknowledgement. */
  canAcknowledge?: boolean;
};
export type ConversationData = {
  id: string;
  title: string;
  started: boolean;
  /** Bound before the first dispatch; retained while admission is unknown or confirmed. */
  node: string | null;
  pinned: boolean;
  archived: boolean;
  draft: Draft;
  messages: Message[];
  execution: Execution;
};
export type ConversationsState = {
  records: ConversationData[];
  error: string | null;
};

/** Admission belongs to the node. Saving history alone never satisfies this contract. */
export type ConversationExecutionApi = {
  submit: (
    input: {
      conversationId: string;
      submissionId: string;
      createConversation: boolean;
      draft: Draft;
      mode: "queue" | "steer";
    },
    signal: AbortSignal,
  ) => Promise<{ accepted: true } | { accepted: false; reason: string }>;
  stop: (conversationId: string, signal: AbortSignal) => Promise<void>;
  /** Explicitly closes uncertain tool records without replaying their effects. */
  acknowledge?: (conversationId: string, signal: AbortSignal) => Promise<void>;
};
