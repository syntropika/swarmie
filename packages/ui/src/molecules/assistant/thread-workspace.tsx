import { useEffect, useId, useRef, useState } from "react";
import type { ComponentProps, ReactNode } from "react";

import { Button } from "../../atoms/button";
import { Icon } from "../../atoms/icon";
import { ApprovalCard } from "./agents";
import { FileAttachment } from "./attachment";
import {
  ComposerRoot,
  ComposerInput,
  ComposerRail,
  ComposerSubmit,
  ComposerAttachments,
  ComposerAttachButton,
} from "./composer";
import { ConversationArrival } from "./conversation-motion";
import { ImageGallery, Sources } from "./knowledge";
import { MessageActions } from "./message-actions";
import { MessageRoot, MessageError, MessageBranches } from "./messages";
import { AssistantLoader } from "./reasoning";
import { MarkdownText } from "./renderers";
import { StreamingText } from "./streaming-text";
import { ThinkingReasoning } from "./thinking-reasoning";
import { ThreadRoot, ThreadViewport, ThreadComposer, ThreadList, ScrollAnchor } from "./thread";
import { useFloatingWorkspace } from "./thread-workspace-state";
import { ToolCall, ToolGroup } from "./tools";
import { VoiceConversation } from "./voice";

/** Host-normalized parts keep provider parsing, execution and toolkit registration outside shared UI. */
export type ThreadMessagePart =
  | { id: string; kind: "text"; text: string; format: "plain" | "markdown"; visibleWords?: number }
  | { id: string; kind: "reasoning"; reasoning: ComponentProps<typeof ThinkingReasoning> }
  | {
      id: string;
      kind: "tool";
      tool: ComponentProps<typeof ToolCall>;
      standalone?: boolean;
      rendered?: ReactNode;
      approval?: ComponentProps<typeof ApprovalCard>;
    }
  | { id: string; kind: "file"; file: ComponentProps<typeof FileAttachment> }
  | { id: string; kind: "images"; gallery: ComponentProps<typeof ImageGallery> }
  | { id: string; kind: "sources"; sources: ComponentProps<typeof Sources> };

type ToolPart = Extract<ThreadMessagePart, { kind: "tool" }>;
type ReasoningPart = Extract<ThreadMessagePart, { kind: "reasoning" }>;
export type ThreadPartGroup =
  | { kind: "tools"; id: string; parts: ToolPart[] }
  | { kind: "reasoning"; id: string; first: ReasoningPart; parts: ReasoningPart[] }
  | { kind: "single"; id: string; part: ThreadMessagePart };

/** Only adjacent equivalent parts group; standalone registered tools retain their authored placement. */
export function groupThreadParts(parts: readonly ThreadMessagePart[]): ThreadPartGroup[] {
  const groups: ThreadPartGroup[] = [];
  for (const part of parts) {
    const previous = groups.at(-1);
    if (part.kind === "tool" && part.standalone !== true) {
      if (previous?.kind === "tools") {
        previous.parts.push(part);
      } else {
        groups.push({ kind: "tools", id: part.id, parts: [part] });
      }
    } else if (part.kind === "reasoning") {
      if (previous?.kind === "reasoning") {
        previous.parts.push(part);
      } else {
        groups.push({ kind: "reasoning", id: part.id, first: part, parts: [part] });
      }
    } else {
      groups.push({ kind: "single", id: part.id, part });
    }
  }
  return groups;
}

export type ThreadMessage = {
  id: string;
  speaker: string;
  authorKind: "human" | "assistant" | "system";
  time?: ReactNode;
  parts: readonly ThreadMessagePart[];
  actions?: ComponentProps<typeof MessageActions>;
  branches?: ComponentProps<typeof MessageBranches>;
} & (
  | { state: "complete" | "streaming" | "cancelled" }
  | { state: "failed"; error: ComponentProps<typeof MessageError> }
);

export type ThreadRendererLabels = {
  transcript: string;
  input: string;
  placeholder: string;
  send: string;
  stop: string;
  loading: string;
  empty: string;
  tools: string;
  attach: string;
  drop: string;
  removeAttachment: string;
  closePreview: string;
  latest: string;
  previewAttachment: (name: string) => string;
};
export type ThreadRendererProps = {
  id: string;
  messages: readonly ThreadMessage[];
  state: "startup" | "loading" | "ready" | "running" | "disabled";
  labels: ThreadRendererLabels;
  draft: string;
  onDraftChange: (draft: string) => void;
  onSubmit: (draft: string) => void;
  onStop: () => void;
  tick: number;
  error?: ComponentProps<typeof MessageError>;
  attachments?: ComponentProps<typeof ComposerAttachments>["files"];
  onRemoveAttachment?: (id: string) => void;
  onFiles?: (files: readonly File[]) => void;
  voice?: ComponentProps<typeof VoiceConversation>;
  composerContext?: ReactNode;
  suggestions?: readonly { id: string; label: string; onSelect: () => void }[];
};

function MessagePart({ part, streaming }: { part: ThreadMessagePart; streaming: boolean }) {
  switch (part.kind) {
    case "text": {
      if (part.format === "markdown") {
        return <MarkdownText text={part.text} />;
      }
      return (
        <StreamingText
          segments={[{ text: part.text }]}
          count={part.visibleWords ?? part.text.split(" ").length}
          streaming={streaming}
          className="min-h-0 max-w-none"
        />
      );
    }
    case "reasoning": {
      return <ThinkingReasoning {...part.reasoning} />;
    }
    case "tool": {
      return (
        <>
          {part.rendered ?? <ToolCall {...part.tool} />}
          {part.approval === undefined ? null : <ApprovalCard {...part.approval} />}
        </>
      );
    }
    case "file": {
      return <FileAttachment {...part.file} source="message" />;
    }
    case "images": {
      return <ImageGallery {...part.gallery} />;
    }
    case "sources": {
      return <Sources {...part.sources} />;
    }
    default: {
      return null;
    }
  }
}

function MessageGroups({ message, toolsLabel }: { message: ThreadMessage; toolsLabel: string }) {
  return groupThreadParts(message.parts).map((group) => {
    if (group.kind === "tools") {
      return (
        <ToolGroup
          key={group.id}
          label={toolsLabel}
          count={group.parts.length}
          active={group.parts.some((part) => part.tool.state === "running")}
          defaultOpen
        >
          {group.parts.map((part) => (
            <div key={part.id} className="space-y-3">
              <MessagePart part={part} streaming={message.state === "streaming"} />
            </div>
          ))}
        </ToolGroup>
      );
    }
    if (group.kind === "reasoning") {
      return (
        <ThinkingReasoning
          key={group.id}
          {...group.first.reasoning}
          sentences={group.parts.flatMap((part) => part.reasoning.sentences)}
          phase={
            group.parts.some((part) => part.reasoning.phase === "thinking")
              ? "thinking"
              : "completed"
          }
        />
      );
    }
    return (
      <MessagePart key={group.id} part={group.part} streaming={message.state === "streaming"} />
    );
  });
}

/** Assembled provider-neutral thread: history, parts, actions, errors, voice and file input share one lifecycle. */
export function ThreadRenderer(props: ThreadRendererProps) {
  const viewport = useRef<HTMLDivElement>(null);
  const [pinState, setPinState] = useState({ id: props.id, pinned: true });
  const pinned = pinState.id !== props.id || pinState.pinned;
  const [dragging, setDragging] = useState(false);
  const busy = props.state === "running";
  const loading = props.state === "loading" || props.state === "startup";
  const disabled = props.state === "disabled" || loading;
  return (
    <ThreadRoot
      className="relative h-full min-h-0"
      aria-label={props.labels.transcript}
      onDragOver={(event) => {
        if (
          !disabled &&
          props.onFiles !== undefined &&
          event.dataTransfer.types.includes("Files")
        ) {
          event.preventDefault();
          setDragging(true);
        }
      }}
      onDragLeave={(event) => {
        if (
          !(event.relatedTarget instanceof Node) ||
          !event.currentTarget.contains(event.relatedTarget)
        ) {
          setDragging(false);
        }
      }}
      onDrop={(event) => {
        setDragging(false);
        if (
          !event.defaultPrevented &&
          !disabled &&
          props.onFiles !== undefined &&
          event.dataTransfer.files.length > 0
        ) {
          event.preventDefault();
          props.onFiles([...event.dataTransfer.files]);
        }
      }}
    >
      {dragging ? (
        <div className="pointer-events-none absolute inset-2 z-20 flex items-center justify-center rounded-panel bg-primary-soft/90 text-ui text-primary">
          {props.labels.drop}
        </div>
      ) : null}
      <ThreadViewport
        key={props.id}
        ref={viewport}
        label={props.labels.transcript}
        onPinnedChange={(next) => {
          setPinState({ id: props.id, pinned: next });
        }}
      >
        {loading ? (
          <ConversationArrival delay={150} duration={200}>
            <AssistantLoader label={props.labels.loading} tick={props.tick} />
          </ConversationArrival>
        ) : null}
        {props.messages.length === 0 && !loading ? (
          <p className="py-6 text-title text-text-muted">{props.labels.empty}</p>
        ) : null}
        {props.messages.map((message) => (
          <ConversationArrival key={message.id} duration={150}>
            <MessageRoot
              speaker={message.speaker}
              authorKind={message.authorKind}
              time={message.time}
              className="space-y-3"
            >
              <MessageGroups message={message} toolsLabel={props.labels.tools} />
              {message.state === "failed" ? <MessageError {...message.error} /> : null}
              {message.branches === undefined ? null : <MessageBranches {...message.branches} />}
              {message.actions === undefined ? null : (
                <ConversationArrival duration={200}>
                  <MessageActions
                    {...message.actions}
                    disabled={message.state === "streaming" || (message.actions.disabled ?? false)}
                  />
                </ConversationArrival>
              )}
            </MessageRoot>
          </ConversationArrival>
        ))}
        {busy && props.messages.at(-1)?.state !== "streaming" ? (
          <AssistantLoader label={props.labels.loading} tick={props.tick} />
        ) : null}
        {props.error === undefined ? null : <MessageError {...props.error} />}
        {props.voice === undefined ? null : <VoiceConversation {...props.voice} />}
        {props.suggestions === undefined ? null : (
          <div className="flex flex-wrap gap-2">
            {props.suggestions.map((suggestion) => (
              <Button
                key={suggestion.id}
                variant="secondary"
                disabled={busy}
                onClick={suggestion.onSelect}
              >
                {suggestion.label}
              </Button>
            ))}
          </div>
        )}
      </ThreadViewport>
      {pinned ? null : (
        <ScrollAnchor
          label={props.labels.latest}
          onClick={() => {
            viewport.current?.scrollTo({ top: viewport.current.scrollHeight, behavior: "instant" });
          }}
        />
      )}
      <ThreadComposer>
        <ComposerRoot
          value={props.draft}
          onValueChange={props.onDraftChange}
          onSubmit={props.onSubmit}
          busy={busy}
          disabled={disabled}
          {...(props.onFiles === undefined ? {} : { onFiles: props.onFiles })}
          dropLabel={props.labels.drop}
        >
          {props.attachments === undefined || props.onRemoveAttachment === undefined ? null : (
            <ComposerAttachments
              files={props.attachments}
              onRemove={props.onRemoveAttachment}
              removeLabel={props.labels.removeAttachment}
              previewLabel={props.labels.previewAttachment}
              closeLabel={props.labels.closePreview}
            />
          )}
          <ComposerInput label={props.labels.input} placeholder={props.labels.placeholder} />
          <ComposerRail>
            {props.composerContext}
            {props.onFiles === undefined ? null : (
              <ComposerAttachButton
                label={props.labels.attach}
                onFiles={props.onFiles}
                disabled={disabled}
              />
            )}
            <ComposerSubmit
              label={props.labels.send}
              stopLabel={props.labels.stop}
              onStop={props.onStop}
            />
          </ComposerRail>
        </ComposerRoot>
      </ThreadComposer>
    </ThreadRoot>
  );
}

export type ThreadWorkspaceNavigation = ComponentProps<typeof ThreadList>;
export type WorkspaceLabels = {
  title: string;
  open: string;
  close: string;
  threads: string;
  newThread: string;
  resize: string;
  resizeHint: string;
  collapse: string;
  expand: string;
  context: string;
};

type WorkspaceBodyProps = {
  thread: ThreadRendererProps;
  navigation?: ThreadWorkspaceNavigation;
  context?: ReactNode;
  labels: WorkspaceLabels;
  onNewThread: () => void;
};

/** Embedded conversation shell with a controlled, reopenable thread list and inspectable context. */
export function ThreadWorkspace({
  thread,
  navigation,
  context,
  labels,
  onNewThread,
  listOpen,
  onListOpenChange,
}: WorkspaceBodyProps & { listOpen: boolean; onListOpenChange: (open: boolean) => void }) {
  return (
    <section
      aria-label={labels.title}
      className="flex h-full min-h-96 min-w-0 flex-col rounded-panel bg-canvas"
    >
      <div className="flex items-center gap-2 p-2">
        <Button
          variant="ghost"
          aria-expanded={listOpen}
          onClick={() => {
            onListOpenChange(!listOpen);
          }}
        >
          <Icon name="sidebar" />
          {listOpen ? labels.collapse : labels.expand}
        </Button>
        <Button variant="ghost" onClick={onNewThread}>
          <Icon name="plus" />
          {labels.newThread}
        </Button>
      </div>
      <div className="flex min-h-0 flex-1">
        {listOpen && navigation !== undefined ? (
          <aside aria-label={labels.threads} className="w-56 max-w-1/2 shrink-0 overflow-auto p-2">
            <ThreadList {...navigation} />
          </aside>
        ) : null}
        <div className="min-w-0 flex-1">
          <ThreadRenderer {...thread} />
        </div>
      </div>
      {context === undefined ? null : (
        <details className="assistant-disclosure px-3 pb-2">
          <summary className="cursor-pointer text-xs text-text-muted">{labels.context}</summary>
          {context}
        </details>
      )}
    </section>
  );
}

/** Distinct nonmodal workspace lifecycle: outside focus stays usable; close restores the launcher. */
export function FloatingAssistantWorkspace({
  open,
  onOpenChange,
  thread,
  navigation,
  context,
  labels,
  onNewThread,
  storageKey = "swarmie.assistant.workspace.size",
  view: controlledView,
  onViewChange,
}: WorkspaceBodyProps & {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  storageKey?: string;
  view?: "thread" | "list";
  onViewChange?: (view: "thread" | "list") => void;
}) {
  const [localView, setLocalView] = useState<"thread" | "list">("thread");
  const view = controlledView ?? localView;
  const changeView = (next: "thread" | "list") => {
    if (controlledView === undefined) {
      setLocalView(next);
    }
    onViewChange?.(next);
  };
  const titleId = useId();
  const hintId = useId();
  const { panel, trigger, handleProps } = useFloatingWorkspace(open, onOpenChange, storageKey);
  useEffect(() => {
    const target =
      view === "thread"
        ? panel.current?.querySelector<HTMLTextAreaElement>("textarea")
        : panel.current?.querySelector<HTMLHeadingElement>("h2");
    if (document.activeElement === document.body && panel.current?.open === true) {
      target?.focus();
    }
  }, [view, panel]);
  return (
    <>
      <Button
        ref={trigger}
        className="fixed right-4 bottom-4 z-40"
        size="icon"
        aria-label={open ? labels.close : labels.open}
        aria-expanded={open}
        aria-controls={titleId}
        onClick={() => {
          if (!open) {
            changeView("thread");
          }
          onOpenChange(!open);
        }}
      >
        <Icon name={open ? "chevron" : "chat"} />
      </Button>
      <dialog
        ref={panel}
        id={titleId}
        aria-labelledby={`${titleId}-heading`}
        className="fixed inset-auto right-4 bottom-20 z-40 m-0 h-(--assistant-workspace-height) max-h-[calc(100dvh-6rem)] w-(--assistant-workspace-width) max-w-[calc(100vw-2rem)] flex-col overflow-hidden rounded-panel border-0 bg-canvas p-0 text-ink shadow-overlay open:flex"
        onCancel={(event) => {
          event.preventDefault();
          onOpenChange(false);
        }}
      >
        <Button
          variant="ghost"
          size="icon-sm"
          {...handleProps}
          aria-label={labels.resize}
          aria-describedby={hintId}
          className="absolute top-0 left-0 z-10 cursor-nwse-resize touch-none"
        >
          <Icon name="arrows-exchange" className="-rotate-45" />
        </Button>
        <span id={hintId} className="sr-only">
          {labels.resizeHint}
        </span>
        <header className="flex shrink-0 items-center gap-1 py-2 pr-2 pl-8">
          <h2
            tabIndex={-1}
            id={`${titleId}-heading`}
            className="min-w-0 flex-1 truncate text-ui font-medium"
          >
            {view === "list" ? labels.threads : labels.title}
          </h2>
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label={labels.threads}
            aria-pressed={view === "list"}
            disabled={navigation === undefined}
            onClick={() => {
              changeView(view === "list" ? "thread" : "list");
            }}
          >
            <Icon name="history" />
          </Button>
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label={labels.newThread}
            onClick={() => {
              onNewThread();
              changeView("thread");
            }}
          >
            <Icon name="plus" />
          </Button>
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label={labels.close}
            onClick={() => {
              onOpenChange(false);
            }}
          >
            <Icon name="close" />
          </Button>
        </header>
        <div className="min-h-0 flex-1" hidden={view === "list"} inert={view === "list"}>
          <ThreadRenderer {...thread} />
        </div>
        {view === "list" && navigation !== undefined ? (
          <div className="min-h-0 flex-1 overflow-auto p-3">
            <ThreadList
              {...navigation}
              onSelect={(id) => {
                navigation.onSelect(id);
                changeView("thread");
              }}
            />
          </div>
        ) : null}
        {context === undefined ? null : (
          <details className="assistant-disclosure px-3 pb-2">
            <summary className="cursor-pointer text-xs text-text-muted">{labels.context}</summary>
            {context}
          </details>
        )}
      </dialog>
    </>
  );
}
