import {
  AssistantRuntimeProvider,
  AuiIf,
  useExternalStoreRuntime,
  ThreadPrimitive,
  ComposerPrimitive,
  MessagePrimitive,
  ActionBarPrimitive,
  useAuiState,
} from "@assistant-ui/react";
import type {
  AppendMessage,
  ImageMessagePartProps,
  ThreadComposerRuntime,
} from "@assistant-ui/react";
import { MarkdownTextPrimitive } from "@assistant-ui/react-markdown";
import {
  ArrowUp,
  Square,
  Copy,
  Check,
  ChevronDown,
  X,
  ListPlus,
  CornerUpLeft,
  Plus,
  Terminal,
  GitFork,
} from "lucide-react";
import {
  useCallback,
  createContext,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import remarkGfm from "remark-gfm";

import { MessageActions } from "@/components/assistant-ui/elements/message-actions";
import { Button } from "@/components/ui/button";
import { Disclosure } from "@/components/ui/disclosure";
import { Tooltip, TooltipContent } from "@/components/ui/tooltip";

import { MediaPreview, OutputAssets } from "./asset-preview";
import { attachmentBytes, readAttachment } from "./attachments";
import { hasCount, hasText, scalarLabel } from "./components/presentation-values";
import { ComposerContext } from "./composer-context";
import { fileLocation, markdownUrl } from "./context";
import { ContributionSlot, useContributions } from "./contribution-host";
import { Plan } from "./plan";
import type { Thread, Tool } from "./protocol";
import { copyText, errorMessage } from "./protocol";
import { Queue } from "./queue";
import { SlashCommands } from "./resource-library";
import type { WorkspaceStore } from "./workspace";

const ActivityContext = createContext<{ tools: Tool[]; running: boolean }>({
  tools: [],
  running: false,
});
const ConversationContext = createContext<{
  thread: Thread;
  store: WorkspaceStore;
} | null>(null);

function ToolActivity() {
  const { tools, running } = useContext(ActivityContext);
  if (tools.length === 0) {
    return null;
  }
  return (
    <Disclosure variant="activity">
      <Disclosure.Summary>
        <span>{running ? "Working" : "Work completed"}</span>
        <span className="muted">
          {tools.length} {tools.length === 1 ? "tool" : "tools"}
        </span>
      </Disclosure.Summary>
      <div className="tool-list">
        {tools.map((tool) => (
          <Disclosure key={tool.id}>
            <Disclosure.Summary>
              <span className={`status-dot ${tool.status === "running" ? "working" : ""}`} />
              {tool.name}
              <span className="muted">{tool.status}</span>
            </Disclosure.Summary>
            {tool.input !== undefined && tool.input !== null && (
              <pre>{JSON.stringify(tool.input, null, 2)}</pre>
            )}
            {tool.output !== undefined && tool.output !== null && (
              <pre>
                {typeof tool.output === "string"
                  ? tool.output
                  : JSON.stringify(tool.output, null, 2)}
              </pre>
            )}
          </Disclosure>
        ))}
      </div>
    </Disclosure>
  );
}

function MarkdownLink({ href, children, ...props }: React.ComponentProps<"a">) {
  const host = useContext(ConversationContext);
  const location = hasText(href) ? fileLocation(href) : undefined;
  return (
    <a
      {...props}
      href={href}
      onClick={(e) => {
        if (location && host) {
          e.preventDefault();
          try {
            host.store.openFile(location);
          } catch (error) {
            host.store.setError(error);
          }
        }
      }}
      target={location ? undefined : "_blank"}
      rel="noreferrer"
    >
      {children}
    </a>
  );
}
function Markdown() {
  return (
    <MarkdownTextPrimitive
      className="markdown"
      remarkPlugins={[remarkGfm]}
      urlTransform={markdownUrl}
      defer
      smooth
      components={{
        a: MarkdownLink,
      }}
    />
  );
}
function MessageImage({ image }: ImageMessagePartProps) {
  return <img className="message-image" src={image} alt="Attachment" />;
}
function useMessageCopy() {
  const host = useContext(ConversationContext);
  const content = useAuiState((state) => state.message.content);
  const [copied, setCopied] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      if (timer.current) {
        clearTimeout(timer.current);
      }
    };
  }, []);
  const handleCopyFailure = (error: unknown) => {
    if (host !== null) {
      host.store.setError(error);
    }
  };
  const copy = () => {
    copyText(
      content
        .filter((part) => part.type === "text")
        .map((part) => part.text)
        .join("\n"),
    )
      .then(() => {
        if (!mounted.current) {
          return;
        }
        setCopied(true);
        if (timer.current) {
          clearTimeout(timer.current);
        }
        timer.current = setTimeout(() => {
          setCopied(false);
        }, 3000);
      })
      .catch(handleCopyFailure);
  };
  return { copied, copy };
}
function UserMessage() {
  const host = useContext(ConversationContext);
  const messageId = useAuiState((s) => s.message.id);
  const { copied, copy } = useMessageCopy();
  const storedDate = host?.thread.messages.find((message) => message.id === messageId)?.createdAt;
  const date = storedDate ? new Date(storedDate) : undefined;
  const validDate = date && Number.isFinite(date.getTime()) ? date : undefined;
  const hasDate = !!validDate;
  const threadId = host?.thread.id;
  const sessionId = host?.thread.sessionId;
  const store = host?.store;
  const connected = store?.getSnapshot().connected;
  useEffect(() => {
    if (
      !hasDate &&
      threadId !== undefined &&
      threadId !== null &&
      threadId !== "" &&
      sessionId !== undefined &&
      sessionId !== null &&
      sessionId !== "" &&
      connected === true
    ) {
      store.loadMessageTimes(threadId).catch(store.setError.bind(store));
    }
  }, [hasDate, threadId, sessionId, connected, store]);
  return (
    <MessagePrimitive.Root className="group/user-message mt-1 mb-8 ml-auto flex w-fit max-w-[88%] min-w-0 flex-col items-end gap-1">
      <div
        data-slot="user-message-bubble"
        className="bg-control-secondary min-w-0 rounded-2xl px-4 py-3 text-sm leading-5 wrap-anywhere whitespace-pre-wrap"
      >
        <MessagePrimitive.Content components={{ Image: MessageImage }} />
      </div>
      <ContributionSlot name="message.details" context={{ messageId }} />
      <ActionBarPrimitive.Root className="text-muted pointer-events-none flex h-7 items-center gap-1 text-xs opacity-0 transition-opacity duration-150 group-focus-within/user-message:pointer-events-auto group-focus-within/user-message:opacity-100 group-hover/user-message:pointer-events-auto group-hover/user-message:opacity-100 motion-reduce:transition-none">
        {validDate && (
          <time
            dateTime={validDate.toISOString()}
            title={validDate.toLocaleString()}
            className="mr-1 tabular-nums"
          >
            {validDate.toLocaleTimeString([], {
              hour: "2-digit",
              minute: "2-digit",
              hour12: false,
            })}
          </time>
        )}
        <Tooltip>
          <Button
            variant="ghost"
            size="icon-sm"
            shape="pill"
            tone={copied ? "success" : "muted"}
            aria-label={copied ? "Copied" : "Copy"}
            onClick={copy}
          >
            {copied ? <Check size={14} /> : <Copy size={14} />}
          </Button>
          <TooltipContent>{copied ? "Copied" : "Copy"}</TooltipContent>
        </Tooltip>
        <ContributionSlot name="message.actions" context={{ messageId }} />
      </ActionBarPrimitive.Root>
    </MessagePrimitive.Root>
  );
}
function AssistantMessage() {
  const host = useContext(ConversationContext);
  const { copied, copy } = useMessageCopy();
  const messageId = useAuiState((s) => s.message.id);
  const [forking, setForking] = useState(false);
  const anchor = host?.thread.messageAnchors?.[messageId];
  return (
    <MessagePrimitive.Root className="assistant-message group/assistant-message">
      <AuiIf condition={(state) => state.message.isLast}>
        <ToolActivity />
      </AuiIf>
      <MessagePrimitive.Content components={{ Text: Markdown }} />
      <AuiIf condition={(state) => !state.message.isLast}>
        <div className="message-divider" />
      </AuiIf>
      <ContributionSlot name="message.details" context={{ messageId }} />
      <ActionBarPrimitive.Root
        className="pointer-events-none mt-3 flex h-7 items-center gap-1 opacity-0 transition-opacity duration-150 group-focus-within/assistant-message:pointer-events-auto group-focus-within/assistant-message:opacity-100 group-hover/assistant-message:pointer-events-auto group-hover/assistant-message:opacity-100 motion-reduce:transition-none"
        hideWhenRunning
      >
        <ContributionSlot name="message.actions" context={{ messageId }} />
        <MessageActions copied={copied} reaction={null} regenerating={false} onCopy={copy} />
        {host?.thread.capabilities?.fork?.availability === "available" &&
          anchor !== undefined &&
          anchor !== null &&
          anchor !== "" &&
          host.thread.state === "idle" && (
            <Tooltip>
              <Button
                shape="pill"
                tone="muted"
                variant="ghost"
                size="icon-sm"

                aria-label="Fork from here"
                disabled={forking}
                onClick={() => {
                  const handlePromiseFailure3 = (error: unknown) => {
                    host?.store.setError(error);
                  };

                  setForking(true);
                  host.store
                    .fork(host.thread.id, anchor, "after_assistant")
                    .catch(host.store.setError.bind(host.store))
                    .finally(() => {
                      setForking(false);
                    })
                    .catch(handlePromiseFailure3);
                }}
              >
                <GitFork size={14} />
              </Button>
              <TooltipContent>Fork from here</TooltipContent>
            </Tooltip>
          )}
      </ActionBarPrimitive.Root>
    </MessagePrimitive.Root>
  );
}

function ConversationTimeline({
  thread,
  store,
  running,
  projectName,
  unresolvedSubmission,
  checkingRecovery,
  checkRecovery,
}: {
  thread: Thread;
  store: WorkspaceStore;
  running: boolean;
  projectName: string | undefined;
  unresolvedSubmission: boolean;
  checkingRecovery: boolean;
  checkRecovery: () => Promise<void>;
}) {
  let runningLabel = "Working on your request…";
  if (thread.state === "needs_input") {
    runningLabel = unresolvedSubmission
      ? "Checking delivery of your request"
      : "Waiting for your permission";
  }
  return (
    <ThreadPrimitive.Viewport className="conversation-scroll" autoScroll>
      <div className="conversation-width">
        <AuiIf condition={(state) => state.thread.isEmpty}>
          <div className="welcome">
            <span className="welcome-mark" aria-hidden="true">
              <Terminal size={30} />
            </span>
            <h1>
              {hasText(projectName)
                ? `What should we build in ${projectName}?`
                : "What should we build?"}
            </h1>
          </div>
        </AuiIf>
        <ThreadPrimitive.Messages>
          {({ message }) => (message.role === "user" ? <UserMessage /> : <AssistantMessage />)}
        </ThreadPrimitive.Messages>
        {hasText(thread.reasoning) && (
          <Disclosure variant="activity">
            <Disclosure.Summary>Thinking</Disclosure.Summary>
            <p className="reasoning-text">{thread.reasoning}</p>
          </Disclosure>
        )}
        {thread.plan !== undefined && thread.plan !== null && <Plan value={thread.plan} />}
        <OutputAssets store={store} files={thread.outputFiles ?? []} />
        {hasCount(thread.subagents?.length) && (
          <Disclosure variant="activity" defaultOpen>
            <Disclosure.Summary>Agents · {thread.subagents.length}</Disclosure.Summary>
            {thread.subagents.map((agent, i) => (
              <div className="subagent-row" key={scalarLabel(agent._id, `agent-${i}`)}>
                <strong>{scalarLabel(agent.name ?? agent.role ?? agent.agentId, "Agent")}</strong>
                <span>{scalarLabel(agent.status ?? agent.state, "Working")}</span>
                {agent.progress !== undefined && agent.progress !== null && (
                  <p>{scalarLabel(agent.progress, "")}</p>
                )}
              </div>
            ))}
          </Disclosure>
        )}
        {running && (
          <div className="running-label">
            <span className="status-dot working" /> {runningLabel}
          </div>
        )}
        {unresolvedSubmission && (
          <output>
            <p>We cannot confirm whether your request finished. Pending work is paused.</p>
            <Button
              variant="ghost"
              disabled={checkingRecovery}
              onClick={() => {
                checkRecovery().catch(store.setError.bind(store));
              }}
            >
              {checkingRecovery ? "Checking…" : "Check request status"}
            </Button>
          </output>
        )}
        {hasText(thread.error) && (
          <p className="inline-error" role="alert">
            {errorMessage(thread.error, thread.harness)}
          </p>
        )}
      </div>
    </ThreadPrimitive.Viewport>
  );
}

export function Conversation({
  thread,
  store,
  controls,
  permissions,
  ready,
  context,
  projectName,
}: {
  thread: Thread;
  store: WorkspaceStore;
  controls: React.ReactNode;
  permissions?: React.ReactNode;
  ready: boolean;
  context?: React.ReactNode;
  projectName?: string;
}) {
  const contributions = useContributions();
  const input = useRef<HTMLTextAreaElement>(null);
  const selection = useRef({ start: 0, end: 0 });
  const upload = useRef<HTMLInputElement>(null);
  const [sending, setSending] = useState(false);
  const [caret, setCaret] = useState(0);
  const [previewAttachment, setPreviewAttachment] = useState<string>();
  const composerBinding = useRef<{ threadId: string; composer: ThreadComposerRuntime } | null>(
    null,
  );
  const onNew = useCallback(
    async (message: AppendMessage) => {
      const text = message.content
        .filter((p) => p.type === "text")
        .map((p) => p.text)
        .join("\n");
      try {
        await store.send(thread.id, text, thread.attachments ?? []);
      } catch (error) {
        store.setError(error);
        const binding = composerBinding.current;
        if (binding?.threadId === thread.id) {
          binding.composer.setText(
            store.getSnapshot().threads.find((t) => t.id === thread.id)?.draft ?? text,
          );
        }
      }
    },
    [store, thread.id, thread.attachments],
  );
  const running = ["starting", "running", "cancelling", "needs_input"].includes(thread.state);
  const runtime = useExternalStoreRuntime({
    messages: thread.messages,
    convertMessage: (message) => message,
    isRunning: running,
    onNew,
    onCancel: async () => {
      try {
        await store.cancel(thread.id);
      } catch (error) {
        store.setError(error);
      }
    },
  });
  const { composer } = runtime.thread;
  useEffect(() => {
    composerBinding.current = { threadId: thread.id, composer };
    return () => {
      composerBinding.current = null;
    };
  }, [thread.id, composer]);
  useEffect(() => {
    composer.setText(thread.draft ?? "");
  }, [thread.draft, composer]);
  useEffect(() => {
    const applyDraft = (event: Event) => {
      if (!(event instanceof CustomEvent)) {
        return;
      }
      const detail: unknown = event.detail;
      if (
        typeof detail === "object" &&
        detail !== null &&
        "threadId" in detail &&
        detail.threadId === thread.id
      ) {
        composer.setText(store.getSnapshot().threads.find((t) => t.id === thread.id)?.draft ?? "");
      }
    };
    window.addEventListener("swarmie:composer-text", applyDraft);
    return () => {
      window.removeEventListener("swarmie:composer-text", applyDraft);
    };
  }, [thread.id, store, composer]);
  useEffect(() => {
    const registry = contributions?.registry;
    if (!registry) {
      return undefined;
    }
    let previous = composer.getState().text;
    return composer.subscribe(() => {
      const { text } = composer.getState();
      if (text === previous) {
        return;
      }
      previous = text;
      if (registry.getContext().conversationId === thread.id) {
        registry.emit({ type: "composer.changed", context: registry.getContext() });
      }
    });
  }, [contributions?.registry, composer, thread.id]);
  useEffect(
    () =>
      contributions?.bindComposer(thread.id, (text, options) => {
        const draft = composer.getState().text;
        const start = Math.min(selection.current.start, draft.length);
        const end = Math.min(selection.current.end, draft.length);
        let inserted = text;
        if (options?.separateWords === true) {
          inserted =
            (start > 0 && !/\s$/u.test(draft.slice(0, start)) ? " " : "") +
            text +
            (end < draft.length && !/^\s/u.test(draft.slice(end)) ? " " : "");
        }
        const next = draft.slice(0, start) + inserted + draft.slice(end);
        composer.setText(next);
        store.updateThread(thread.id, { draft: next });
        selection.current = { start: start + inserted.length, end: start + inserted.length };
        requestAnimationFrame(() => {
          if (input.current?.isConnected === true) {
            input.current.focus();
            input.current.setSelectionRange(start + inserted.length, start + inserted.length);
          }
        });
      }),
    [contributions, thread.id, composer, store],
  );
  const [checkingRecovery, setCheckingRecovery] = useState(false);
  const unresolvedSubmission = thread.submissionKeys?.some(
    (entry) =>
      !entry.receipt ||
      entry.receipt.recoveryRequired ||
      !["accepted", "rejected"].includes(entry.receipt.delivery),
  );
  const checkRecovery = async () => {
    setCheckingRecovery(true);
    try {
      await store.checkSubmissionRecovery(thread.id);
    } catch (error) {
      store.setError(error);
    } finally {
      setCheckingRecovery(false);
    }
  };
  const submitDuringTurn = async (delivery: "queue" | "steer") => {
    setSending(true);
    try {
      await store.send(thread.id, thread.draft ?? "", thread.attachments ?? [], delivery);
      composer.setText(store.getSnapshot().threads.find((t) => t.id === thread.id)?.draft ?? "");
    } catch (error) {
      store.setError(error);
    } finally {
      setSending(false);
    }
  };
  const attach = async (files: File[]) => {
    try {
      const next = await Promise.all(files.map(readAttachment));
      if (
        [...(thread.attachments ?? []), ...next].reduce(
          (sum, file) => sum + attachmentBytes(file),
          0,
        ) >
        8 * 1024 * 1024
      ) {
        throw new Error("Keep total attached content under 8 MB.");
      }
      if ((thread.attachments?.length ?? 0) + next.length > 8) {
        throw new Error("Attach up to eight files per message.");
      }
      store.updateThread(thread.id, {
        attachments: [...(thread.attachments ?? []), ...next],
      });
    } catch (error) {
      store.setError(error);
    }
  };
  const conversationHost = useMemo(() => ({ thread, store }), [thread, store]);
  const activity = useMemo(() => ({ tools: thread.tools, running }), [thread.tools, running]);
  return (
    <AssistantRuntimeProvider runtime={runtime}>
      <ConversationContext.Provider value={conversationHost}>
        <ActivityContext.Provider value={activity}>
          <ThreadPrimitive.Root
            className={`conversation ${thread.messages.length === 0 && !running ? "conversation-empty" : ""}`}
          >
            <div className="conversation-contribution-header">
              <ContributionSlot name="conversation.header.actions" />
            </div>
            <ConversationTimeline
              thread={thread}
              store={store}
              running={running}
              projectName={projectName}
              unresolvedSubmission={unresolvedSubmission === true}
              checkingRecovery={checkingRecovery}
              checkRecovery={checkRecovery}
            />
            <div className="composer-dock">
              <div className="conversation-width">
                <ThreadPrimitive.ScrollToBottom asChild>
                  <Button size="icon" variant="reveal" aria-label="Jump to latest message">
                    <ChevronDown size={17} />
                  </Button>
                </ThreadPrimitive.ScrollToBottom>
                <Queue thread={thread} store={store} />
                <div className="composer-context-bar">
                  {context}
                  <ContributionSlot name="composer.context" />
                </div>
                <ComposerPrimitive.Root
                  className="composer"
                  onDragOver={(e) => {
                    e.preventDefault();
                  }}
                  onDrop={(e) => {
                    e.preventDefault();
                    attach([...e.dataTransfer.files]).catch(store.setError.bind(store));
                  }}
                >
                  <ComposerContext
                    thread={thread}
                    store={store}
                    caret={caret}
                    setText={(text) => {
                      composer.setText(text);
                    }}
                  />
                  <SlashCommands
                    thread={thread}
                    store={store}
                    setText={(text) => {
                      composer.setText(text);
                    }}
                  />
                  {hasCount(thread.attachments?.length) && (
                    <div className="attachments">
                      {thread.attachments.map((file) => (
                        <div className="attachment" key={file.id}>
                          {file.image && (
                            <img src={`data:${file.mime};base64,${file.data}`} alt={file.name} />
                          )}
                          <Button
                            size="sm"
                            variant="ghost"

                            onClick={() => {
                              setPreviewAttachment(
                                previewAttachment === file.id ? undefined : file.id,
                              );
                            }}
                          >
                            {file.name}
                          </Button>
                          <Button
                            size="icon"
                            variant="ghost"

                            aria-label={`Remove ${file.name}`}
                            onClick={() => {
                              store.updateThread(thread.id, {
                                attachments: thread.attachments?.filter((a) => a.id !== file.id),
                              });
                            }}
                          >
                            <X size={13} />
                          </Button>
                        </div>
                      ))}
                    </div>
                  )}
                  {thread.attachments
                    ?.filter((a) => a.id === previewAttachment && (a.image || a.binary === true))
                    .map((a) => (
                      <MediaPreview key={a.id} asset={a} store={store} />
                    ))}
                  <input
                    ref={upload}
                    type="file"
                    multiple
                    hidden
                    aria-label="Attach files"
                    onChange={(e) => {
                      attach([...(e.target.files ?? [])]).catch(store.setError.bind(store));
                      e.target.value = "";
                    }}
                  />
                  <ComposerPrimitive.Input
                    className="composer-input"
                    ref={input}
                    onBlur={(event) => {
                      selection.current = {
                        start: event.currentTarget.selectionStart,
                        end: event.currentTarget.selectionEnd,
                      };
                    }}
                    placeholder="Ask anything, or describe a task…"
                    aria-label="Message"
                    disabled={!ready}
                    rows={2}
                    onChange={(e) => {
                      setCaret(e.target.selectionStart);
                      selection.current = {
                        start: e.target.selectionStart,
                        end: e.target.selectionEnd,
                      };
                      store.updateThread(thread.id, { draft: e.target.value });
                    }}
                    onSelect={(e) => {
                      setCaret(e.currentTarget.selectionStart);
                      selection.current = {
                        start: e.currentTarget.selectionStart,
                        end: e.currentTarget.selectionEnd,
                      };
                    }}
                    onPaste={(e) => {
                      if (e.clipboardData.files.length) {
                        e.preventDefault();
                        attach([...e.clipboardData.files]).catch(store.setError.bind(store));
                      }
                    }}
                    autoFocus
                  />
                  <div className="composer-toolbar">
                    <Button
                      shape="pill"
                      variant="ghost"
                      size="icon-sm"

                      aria-label="Attach files or images"
                      disabled={!ready}
                      onClick={() => upload.current?.click()}
                    >
                      <Plus size={18} />
                    </Button>
                    {permissions}
                    <div className="composer-spacer" />
                    <div className="composer-send-actions">
                      {controls}
                      <ContributionSlot name="composer.actions" />
                      <AuiIf condition={(state) => !state.thread.isRunning}>
                        <ComposerPrimitive.Send asChild>
                          <Button
                            shape="pill"
                            variant="send"
                            size="icon-sm"

                            aria-label="Send message"
                            type="submit"
                            disabled={!ready}
                          >
                            <ArrowUp size={18} />
                          </Button>
                        </ComposerPrimitive.Send>
                      </AuiIf>
                      <AuiIf condition={(state) => state.thread.isRunning}>
                        <Button
                          variant="ghost"
                          size="sm"

                          disabled={
                            sending ||
                            !(hasText(thread.draft?.trim()) || hasCount(thread.attachments?.length))
                          }
                          onClick={() => {
                            submitDuringTurn("queue").catch(store.setError.bind(store));
                          }}
                        >
                          <ListPlus size={14} />
                          Queue
                        </Button>
                        {thread.capabilities?.steering?.availability === "available" && (
                          <Button
                            variant="ghost"
                            size="sm"

                            disabled={sending || !hasText(thread.draft?.trim())}
                            onClick={() => {
                              submitDuringTurn("steer").catch(store.setError.bind(store));
                            }}
                          >
                            <CornerUpLeft size={14} />
                            Steer
                          </Button>
                        )}
                        <ComposerPrimitive.Cancel asChild>
                          <Button
                            shape="pill"
                            variant="stop"
                            size="icon-sm"

                            aria-label="Stop generation"
                          >
                            <Square size={13} fill="currentColor" />
                          </Button>
                        </ComposerPrimitive.Cancel>
                      </AuiIf>
                    </div>
                  </div>
                  <ContributionSlot name="composer.status" />
                </ComposerPrimitive.Root>
              </div>
            </div>
          </ThreadPrimitive.Root>
        </ActivityContext.Provider>
      </ConversationContext.Provider>
    </AssistantRuntimeProvider>
  );
}

export function PatchCopy({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);
  const [failed, setFailed] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  useEffect(
    () => () => {
      if (timer.current !== undefined) {
        clearTimeout(timer.current);
      }
    },
    [],
  );
  const copyPatch = async () => {
    try {
      await copyText(text);
      setCopied(true);
      setFailed(false);
      timer.current = setTimeout(() => {
        setCopied(false);
      }, 1800);
    } catch {
      setFailed(true);
    }
  };
  const reportCopyFailure = () => {
    setFailed(true);
  };
  return (
    <span>
      <Button
        variant="ghost"
        size="sm"
        onClick={() => {
          copyPatch().catch(reportCopyFailure);
        }}
      >
        {copied ? <Check size={14} /> : <Copy size={14} />} {copied ? "Copied" : "Copy patch"}
      </Button>
      {failed && <span role="alert">The patch could not be copied.</span>}
    </span>
  );
}
