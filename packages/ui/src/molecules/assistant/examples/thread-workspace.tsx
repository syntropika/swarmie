import { useState } from "react";

import { Button } from "../../../atoms/button";
import type { ComposerAttachment } from "../composer";
import type { MessageActionsLabels, MessageVote } from "../message-actions";
import { FloatingAssistantWorkspace, ThreadRenderer, ThreadWorkspace } from "../thread-workspace";
import type {
  ThreadMessage,
  ThreadMessagePart,
  ThreadRendererLabels,
  ThreadRendererProps,
  WorkspaceLabels,
} from "../thread-workspace";
import { demoImage, sources } from "./fixtures";

const threadLabels: ThreadRendererLabels = {
  transcript: "Conversation",
  input: "Message",
  placeholder: "Ask about this workspace",
  send: "Send message",
  stop: "Stop response",
  loading: "Loading conversation",
  empty: "How can I help?",
  tools: "Tool activity",
  attach: "Attach files",
  drop: "Drop files to attach",
  removeAttachment: "Remove file",
  closePreview: "Close preview",
  latest: "Jump to latest",
  previewAttachment: (name) => `Preview ${name}`,
};
const shellLabels: WorkspaceLabels = {
  title: "Workspace assistant",
  open: "Open assistant workspace",
  close: "Close assistant workspace",
  threads: "Browse conversations",
  newThread: "New conversation",
  resize: "Resize assistant workspace",
  resizeHint: "Arrow keys resize, Shift resizes faster. Enter or Space resets the size.",
  collapse: "Collapse conversations",
  expand: "Show conversations",
  context: "Workspace context",
};
const actionLabels: MessageActionsLabels = {
  good: "Helpful",
  bad: "Not helpful",
  copy: "Copy response",
  copied: "Copied response",
  react: "React",
  reaction: (emoji) => `React ${emoji}`,
  reply: "Reply",
  more: "More actions",
  aloud: "Read aloud",
  fork: "Fork",
  report: "Report",
  search: "Search reactions",
  empty: "No reactions",
  back: "Back",
};
type DemoStage = "empty" | "approval" | "running" | "complete" | "failed" | "cancelled";

const stagePresentation = {
  empty: { tool: "waiting", label: "Waiting", message: "complete", thread: "ready" },
  approval: { tool: "waiting", label: "Needs approval", message: "complete", thread: "ready" },
  running: { tool: "running", label: "Running", message: "streaming", thread: "running" },
  complete: { tool: "completed", label: "Completed", message: "complete", thread: "ready" },
  failed: { tool: "failed", label: "Failed", message: "complete", thread: "ready" },
  cancelled: { tool: "cancelled", label: "Cancelled", message: "cancelled", thread: "ready" },
} as const;

/** A synthetic host controller changes real parts and shell state without a provider runtime. */
export function ThreadWorkspaceExample({
  presentation = "floating",
}: {
  presentation?: "floating" | "embedded" | "thread";
}) {
  const [open, setOpen] = useState(false);
  const [listOpen, setListOpen] = useState(true);
  const [activeId, setActiveId] = useState("review");
  const [items, setItems] = useState([
    { id: "review", title: "Component review", detail: "Current work", group: "Today" },
    { id: "notes", title: "Research notes", detail: "Earlier conversation", group: "Yesterday" },
  ]);
  const [draft, setDraft] = useState("");
  const [prompt, setPrompt] = useState("Review this component.");
  const [stage, setStage] = useState<DemoStage>("complete");
  const [loading, setLoading] = useState(false);
  const [files, setFiles] = useState<ComposerAttachment[]>([]);
  const [submittedFiles, setSubmittedFiles] = useState<ComposerAttachment[]>([]);
  const [copied, setCopied] = useState(false);
  const [vote, setVote] = useState<MessageVote>(null);
  const [reaction, setReaction] = useState<string | null>(null);
  const [tick, setTick] = useState(1);
  const busy = stage === "running";
  const state = stagePresentation[stage];
  const result =
    stage === "cancelled"
      ? "The response was stopped. Your message is still available."
      : "The component has accessible controls and shared styling.";
  const approval = {
    state: "request" as const,
    title: "Read workspace files?",
    subtitle: "The assistant needs the current component contract",
    allowOnceLabel: "Allow once",
    alwaysAllowLabel: "Always allow",
    denyLabel: "Decline",
    statusLabel: "Waiting for your decision",
    onAllowOnce: () => {
      setStage("running");
    },
    onAlwaysAllow: () => {
      setStage("running");
    },
    onDeny: () => {
      setStage("cancelled");
    },
  };
  const parts: ThreadMessagePart[] = [
    {
      id: "reasoning",
      kind: "reasoning",
      reasoning: {
        sentences: [
          { id: "scope", text: "Inspect the shared contract and verify the interactions." },
        ],
        phase: busy ? "thinking" : "completed",
        thinkingLabel: "Thinking",
        completedLabel: "Reasoning",
        toggleLabel: "Toggle reasoning",
      },
    },
    {
      id: "tool",
      kind: "tool",
      tool: {
        name: "Read component contract",
        activeLabel: "Reading component contract",
        state: state.tool,
        statusLabel: state.label,
        request: <code>frontend.md</code>,
        result: busy ? undefined : <p>Controls receive typed callbacks.</p>,
        requestLabel: "Request",
        resultLabel: "Result",
      },
      ...(stage === "approval" ? { approval } : {}),
    },
  ];
  if (stage === "complete") {
    parts.push(
      { id: "text", kind: "text", text: result, format: "markdown" },
      { id: "sources", kind: "sources", sources: { label: "Sources", sources } },
      {
        id: "image",
        kind: "images",
        gallery: {
          label: "Preview",
          images: [
            {
              id: "preview",
              src: demoImage,
              alt: "Component illustration",
              caption: "Component illustration",
              state: "loaded",
            },
          ],
        },
      },
    );
  } else if (stage === "running") {
    parts.push({
      id: "stream",
      kind: "text",
      text: "Checking focus labels and keyboard navigation",
      format: "plain",
      visibleWords: tick,
    });
  } else if (stage === "cancelled") {
    parts.push({ id: "cancelled", kind: "text", text: result, format: "plain" });
  }
  const actions = {
    label: "Response actions",
    labels: actionLabels,
    text: result,
    vote,
    onVote: setVote,
    reaction,
    onReact: setReaction,
    copied,
    onCopy: () => {
      setCopied(true);
    },
    onReply: () => {
      setDraft(`Regarding: ${result}`);
    },
    onRegenerate: () => {
      setStage("running");
      setCopied(false);
    },
    regenerateLabel: "Regenerate response",
    regeneratingLabel: "Regenerating",
    regenerating: busy,
  };
  const assistant: ThreadMessage =
    stage === "failed"
      ? {
          id: "assistant",
          speaker: "Swarmie",
          authorKind: "assistant",
          state: "failed",
          parts,
          error: {
            title: "Response interrupted",
            message: "The demo source became unavailable.",
            retryLabel: "Retry response",
            onRetry: () => {
              setStage("running");
            },
          },
        }
      : {
          id: "assistant",
          speaker: "Swarmie",
          authorKind: "assistant",
          state: state.message,
          parts,
          actions,
        };
  let messages: readonly ThreadMessage[] = [];
  if (activeId === "notes") {
    messages = [
      {
        id: "notes-message",
        speaker: "Swarmie",
        authorKind: "assistant",
        state: "complete",
        parts: [
          {
            id: "notes-text",
            kind: "text",
            text: "Earlier notes: keep execution in the host adapter.",
            format: "markdown",
          },
        ],
      },
    ];
  } else if (stage !== "empty") {
    messages = [
      {
        id: "human",
        speaker: "You",
        authorKind: "human",
        state: "complete",
        parts: [
          { id: "prompt", kind: "text", text: prompt, format: "plain" },
          ...submittedFiles.map((file): ThreadMessagePart => ({
            id: file.id,
            kind: "file",
            file: { name: file.name, detail: file.detail },
          })),
        ],
      },
      assistant,
    ];
  }
  const thread: ThreadRendererProps = {
    id: activeId,
    messages,
    state: loading ? "loading" : state.thread,
    labels: threadLabels,
    draft,
    onDraftChange: setDraft,
    tick,
    onSubmit: (value) => {
      setPrompt(value);
      setDraft("");
      setStage("approval");
      setSubmittedFiles(files);
      setFiles([]);
    },
    onStop: () => {
      setStage("cancelled");
    },
    attachments: files,
    onRemoveAttachment: (id) => {
      setFiles((current) => current.filter((file) => file.id !== id));
    },
    onFiles: (incoming) => {
      setFiles((current) => [
        ...current,
        ...incoming.map((file) => ({
          id: `${file.name}-${file.lastModified}`,
          name: file.name,
          detail: `${file.size} bytes`,
          status: "ready" as const,
        })),
      ]);
    },
  };
  const navigation = {
    label: "Conversations",
    items,
    activeId,
    onSelect: (id: string) => {
      setActiveId(id);
      setDraft("");
    },
  };
  const onNewThread = () => {
    const id = `conversation-${items.length}`;
    setItems((current) => [
      ...current,
      { id, title: `Conversation ${items.length}`, detail: "New conversation", group: "Today" },
    ]);
    setActiveId(id);
    setStage("empty");
    setDraft("");
    setFiles([]);
  };
  const common = {
    thread,
    navigation,
    labels: shellLabels,
    onNewThread,
    context: (
      <p className="text-xs text-text-muted">
        Demo workspace · Shared UI contracts · Local host adapter
      </p>
    ),
  };
  let workspace = <ThreadRenderer {...thread} />;
  if (presentation === "floating") {
    workspace = (
      <FloatingAssistantWorkspace
        {...common}
        open={open}
        onOpenChange={setOpen}
        storageKey="swarmie.storybook.workspace-size"
      />
    );
  }
  if (presentation === "embedded") {
    workspace = <ThreadWorkspace {...common} listOpen={listOpen} onListOpenChange={setListOpen} />;
  }
  return (
    <div className="space-y-3">
      <div className="flex flex-wrap gap-2">
        <Button
          variant="secondary"
          onClick={() => {
            setStage("running");
          }}
        >
          Start streaming
        </Button>
        <Button
          variant="secondary"
          onClick={() => {
            setTick((value) => value + 1);
          }}
        >
          Advance stream
        </Button>
        <Button
          variant="secondary"
          onClick={() => {
            setStage("complete");
          }}
        >
          Finish response
        </Button>
        <Button
          variant="ghost"
          onClick={() => {
            setStage("failed");
          }}
        >
          Fail response
        </Button>
        <Button
          variant="ghost"
          onClick={() => {
            setLoading(!loading);
          }}
        >
          {loading ? "Finish loading history" : "Load history"}
        </Button>
      </div>
      <div className="h-[36rem] min-h-0">{workspace}</div>
    </div>
  );
}
