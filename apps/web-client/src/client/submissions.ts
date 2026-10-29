import type { Snapshot } from "@swarmie/client-ui";
import type {
  Connections,
  ConnectionsState,
  NodeConnection,
} from "@swarmie/plugin-connections-browser";
import type {
  Conversation,
  ConversationExecutionApi,
  ConversationExecutionContext,
  createConversationRecords,
  ConversationsUiApi,
  Execution,
} from "@swarmie/plugin-conversations-browser";
import { defaultConversationTitle } from "@swarmie/plugin-conversations-browser";
import type { ProvidersApi } from "@swarmie/plugin-providers-browser";

import { sameExecutionContext } from "./conversation-values";

export type ExecutionFactory = (
  connection: NodeConnection,
) => ConversationExecutionApi | Promise<ConversationExecutionApi>;

/** Runs before node admission; setup must finish before this boundary returns. */
export type SubmissionAdmission = (input: {
  conversationId: string;
  submissionId: string;
  createConversation: boolean;
  context: ConversationExecutionContext | undefined;
  connection: NodeConnection;
  signal: AbortSignal;
}) => Promise<ConversationExecutionContext | undefined>;

type StopBatch = {
  pending: number;
  committing: number;
  previous: Execution;
  unknownAdmission: boolean;
  succeeded: boolean;
};

function conversationNode(chat: Conversation) {
  return chat.node ?? chat.draft.getSnapshot().node;
}

function removeQueued(chat: Conversation, id: string) {
  chat.execution.update((current) => ({
    ...current,
    queue: current.queue.filter((item) => item.id !== id),
    steering: current.steering.filter((item) => item.id !== id),
  }));
}

/** Keeps destination binding and admission uncertainty in one operation owner. */
export function createSubmissions(
  connections: Connections,
  nodes: Snapshot<Readonly<ConnectionsState>>,
  records: ReturnType<typeof createConversationRecords>,
  providers: ProvidersApi,
  execution: ExecutionFactory | undefined,
  lifecycle: ConversationsUiApi["lifecycle"],
  nextId: () => string,
  capability: {
    available: (nodeId: string) => boolean;
    subscribe: (changed: () => void) => () => void;
    limits: { maxTextBytes: number; attachments: boolean };
  } | null = null,
  beforeSubmit?: SubmissionAdmission,
) {
  const pending = new Set<string>();
  const stopBatches = new Map<string, StopBatch>();
  const failed = records.fail;
  const limits = capability?.limits;
  function available(nodeId: string) {
    return capability?.available(nodeId) ?? true;
  }
  function submissionIssue(chat: Conversation): string | null {
    if (limits === undefined) {
      return null;
    }
    const draft = chat.draft.getSnapshot();
    if (!limits.attachments && draft.attachments.length > 0) {
      return "Attachments are not supported by this node. Remove them before sending.";
    }
    return new TextEncoder().encode(draft.text).length > limits.maxTextBytes
      ? `This node accepts messages up to ${limits.maxTextBytes} bytes. Shorten the message before sending.`
      : null;
  }

  function setConversationNode(chat: Conversation, nodeId: string) {
    if (
      chat.node !== null ||
      pending.has(chat.id) ||
      chat.execution.getSnapshot().phase === "unknown"
    ) {
      return;
    }
    if (!nodes.getSnapshot().nodes.some((node) => node.id === nodeId)) {
      throw new Error("The execution node is not configured.");
    }
    chat.draft.update((draft) => ({ ...draft, node: nodeId, executionContext: undefined }));
  }
  function setConversationContext(chat: Conversation, context: ConversationExecutionContext) {
    if (
      chat.started ||
      chat.node !== null ||
      pending.has(chat.id) ||
      chat.execution.getSnapshot().phase === "unknown"
    ) {
      throw new Error("The conversation execution directory is already bound.");
    }
    if (
      !nodes
        .getSnapshot()
        .nodes.some(
          (node) =>
            node.id === context.nodeId &&
            (context.principal === undefined || node.principal === context.principal),
        ) ||
      context.path.trim() === ""
    ) {
      throw new Error("Choose a configured node and an execution directory.");
    }
    const captured = structuredClone(context);
    chat.draft.update((draft) => ({ ...draft, node: captured.nodeId, executionContext: captured }));
  }
  function canSend(chat?: Conversation) {
    if (execution === undefined) {
      return false;
    }
    if (chat === undefined) {
      return true;
    }
    const draft = chat.draft.getSnapshot();
    return (
      draft.text.trim() !== "" &&
      submissionIssue(chat) === null &&
      available(conversationNode(chat)) &&
      !pending.has(chat.id) &&
      (chat.started || chat.node === null) &&
      nodes.getSnapshot().nodes.some((node) => node.id === conversationNode(chat)) &&
      providers.resolve(draft).available &&
      chat.execution.getSnapshot().phase !== "unknown"
    );
  }
  function send(chat: Conversation, mode: "queue" | "steer" = "queue") {
    if (execution === undefined) {
      failed(new Error("Sending is not available from this node yet."));
      return;
    }
    const draft = chat.draft.getSnapshot();
    if (!canSend(chat)) {
      return;
    }
    const controller = new AbortController();
    pending.add(chat.id);
    const factory = execution;
    const destination = conversationNode(chat);
    let executionContext = structuredClone(chat.executionContext ?? draft.executionContext);
    const submissionId = nextId();
    let dispatched = false;
    let boundHere = false;
    let source: NodeConnection | null = null;
    async function submit() {
      source = await connections.open(destination);
      if (executionContext && executionContext.nodeId !== destination) {
        throw new Error("The execution directory belongs to another node.");
      }
      if (
        executionContext?.principal !== undefined &&
        executionContext.principal !== source.principal
      ) {
        throw new Error("The execution directory was selected under another node user.");
      }
      if (beforeSubmit) {
        executionContext = await beforeSubmit({
          conversationId: chat.id,
          submissionId,
          createConversation: !chat.started,
          context: executionContext,
          connection: source,
          signal: controller.signal,
        });
      }
      if (executionContext && executionContext.nodeId !== destination) {
        throw new Error("Admission changed the execution node.");
      }
      if (chat.executionContext && !sameExecutionContext(executionContext, chat.executionContext)) {
        throw new Error("Admission changed the bound conversation directory.");
      }
      const api = await factory(source);
      if (!source.isCurrent()) {
        throw new Error("The node session ended before submission.");
      }
      if (chat.node === null) {
        await records.commit(chat.id, (record) => ({
          ...record,
          node: destination,
          executionContext,
        }));
        boundHere = true;
      }
      if (!source.isCurrent()) {
        throw new Error("The node session ended before submission.");
      }
      dispatched = true;
      const result = await api.submit(
        {
          conversationId: chat.id,
          submissionId,
          createConversation: !chat.started,
          draft: {
            ...draft,
            attachments: [...draft.attachments],
            node: destination,
            executionContext,
          },
          mode,
          ...(executionContext ? { executionContext } : {}),
        },
        controller.signal,
      );
      if (!source.isCurrent()) {
        throw new Error("The submission result belongs to an earlier node session.");
      }
      return result;
    }
    submit()
      .then(async (result) => {
        if (result.accepted) {
          await accepted();
          return;
        }
        // Only a typed response from the current node proves that no work was admitted.
        dispatched = false;
        submissionFailed(new Error(result.reason));
      })
      .catch(submissionFailed);
    async function accepted() {
      try {
        if (!chat.started) {
          await records.commit(chat.id, (record) => ({
            ...record,
            started: true,
            node: destination,
            executionContext,
            title:
              record.title === defaultConversationTitle
                ? draft.text.trim().slice(0, 64)
                : record.title,
          }));
          lifecycle.created(chat.id);
        }
        chat.draft.update((current) =>
          current === draft
            ? { ...current, node: destination, text: "", attachments: [] }
            : { ...current, node: destination },
        );
        stopBatches.delete(chat.id);
        chat.execution.update((current) =>
          current.phase === "completed" ||
          current.phase === "stopped" ||
          current.phase === "failed" ||
          current.phase === "unknown"
            ? current
            : { ...current, phase: "working", label: "Thinking…" },
        );
      } catch (error: unknown) {
        failed(error);
      } finally {
        pending.delete(chat.id);
      }
    }
    function submissionFailed(error: unknown) {
      if (!dispatched && boundHere) {
        records.update(chat.id, (record) =>
          record.started ? record : { ...record, node: null, executionContext: undefined },
        );
      }
      stopBatches.delete(chat.id);
      chat.execution.update((current) => ({
        ...current,
        phase: dispatched ? "unknown" : "idle",
        label: dispatched ? "Checking whether the message was accepted…" : "",
      }));
      pending.delete(chat.id);
      failed(error);
    }
  }
  function stop(chat: Conversation) {
    if (execution === undefined || (!chat.started && chat.node === null)) {
      return;
    }
    const previous = chat.execution.getSnapshot();
    const active = stopBatches.get(chat.id);
    // Same-turn calls can precede the owner's first stopping update.
    const batch: StopBatch =
      active && (previous.phase === "stopping" || previous === active.previous)
        ? active
        : {
            pending: 0,
            committing: 0,
            previous,
            unknownAdmission: !chat.started && chat.node !== null,
            succeeded: false,
          };
    stopBatches.set(chat.id, batch);
    batch.pending += 1;
    const controller = new AbortController();
    chat.execution.update((current) =>
      stopBatches.get(chat.id) !== batch || (current.phase === "unknown" && !batch.unknownAdmission)
        ? current
        : { ...current, phase: "stopping", label: "Stopping…" },
    );
    const factory = execution;
    const destination = conversationNode(chat);
    async function requestStop() {
      const session = await connections.open(destination);
      const api = await factory(session);
      if (!session.isCurrent()) {
        throw new Error("The node session ended before cancellation.");
      }
      await api.stop(chat.id, controller.signal);
      if (!session.isCurrent()) {
        throw new Error("The cancellation result belongs to an earlier node session.");
      }
    }
    async function settle(succeeded: boolean) {
      batch.pending -= 1;
      batch.succeeded ||= succeeded;
      batch.committing += 1;
      try {
        await records.commit(chat.id, (record) => {
          if (
            stopBatches.get(chat.id) !== batch ||
            record.execution.phase !== "stopping" ||
            (!batch.succeeded && batch.pending > 0)
          ) {
            return record;
          }
          let { phase, label } = batch.previous;
          if (batch.unknownAdmission) {
            phase = "unknown";
            label = "Checking whether the message was accepted…";
          } else if (batch.succeeded) {
            phase = "stopped";
            label = "Stopped";
          }
          return { ...record, execution: { ...record.execution, phase, label } };
        });
      } finally {
        batch.committing -= 1;
        if (batch.pending === 0 && batch.committing === 0 && stopBatches.get(chat.id) === batch) {
          stopBatches.delete(chat.id);
        }
      }
    }
    async function finish() {
      let succeeded = false;
      try {
        await requestStop();
        succeeded = true;
      } catch (error: unknown) {
        failed(error);
      }
      await settle(succeeded);
    }
    finish().catch(failed);
  }
  function resumeQueued() {
    failed(new Error("Resuming queued messages requires a node admission service."));
  }
  async function acknowledgeUnknown(chat: Conversation) {
    const current = chat.execution.getSnapshot();
    if (execution === undefined || current.phase !== "unknown" || current.canAcknowledge !== true) {
      return;
    }
    const connection = await connections.open(conversationNode(chat));
    const api = await execution(connection);
    if (!connection.isCurrent() || api.acknowledge === undefined) {
      throw new Error("The node cannot acknowledge this outcome.");
    }
    await api.acknowledge(chat.id, new AbortController().signal);
    if (!connection.isCurrent()) {
      throw new Error("The acknowledgement belongs to an earlier node session.");
    }
  }
  return {
    conversationNode,
    setConversationNode,
    setConversationContext,
    canSend,
    subscribeExecution: capability?.subscribe ?? nodes.subscribe,
    send,
    stop,
    removeQueued,
    resumeQueued,
    acknowledgeUnknown,
    submissionIssue,
  };
}
