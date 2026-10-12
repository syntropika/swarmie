import { Disclosure } from "@/components/ui/disclosure";
import {
  useCallback,
  createContext,
  useContext,
  useEffect,
  useRef,
} from "react";
import {
  AssistantRuntimeProvider,
  useExternalStoreRuntime,
  ThreadPrimitive,
  ComposerPrimitive,
  MessagePrimitive,
  ActionBarPrimitive,
  useAuiState,
  type AppendMessage,
  type ImageMessagePartProps,
} from "@assistant-ui/react";
import { MarkdownTextPrimitive } from "@assistant-ui/react-markdown";
import remarkGfm from "remark-gfm";
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
import { useState } from "react";
import { Button } from "@/components/ui/button";
import type { Thread, Tool } from "./protocol";
import { copyText, errorMessage } from "./protocol";
import { attachmentBytes, readAttachment } from "./attachments";
import { Plan } from "./Plan";
import { Queue } from "./Queue";
import type { WorkspaceStore } from "./workspace";
import { fileLocation, markdownUrl } from "./context";
import { ComposerContext } from "./ComposerContext";
import { SlashCommands } from "./ResourceLibrary";
import { MediaPreview, OutputAssets } from "./AssetPreview";
import { Tooltip, TooltipContent } from "@/components/ui/tooltip";
import { MessageActions } from "@/components/assistant-ui/elements/message-actions";
import { VoiceInput } from "./VoiceInput";

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
  return (
    <>
      {tools.length > 0 && (
        <Disclosure className="tool-activity">
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
                  <span
                    className={`status-dot ${tool.status === "running" ? "working" : ""}`}
                  />
                  {tool.name}
                  <span className="muted">{tool.status}</span>
                </Disclosure.Summary>
                {tool.input != null && (
                  <pre>{JSON.stringify(tool.input, null, 2)}</pre>
                )}
                {tool.output != null && (
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
      )}
    </>
  );
}

function Markdown() {
  const host = useContext(ConversationContext);
  return (
    <MarkdownTextPrimitive
      className="markdown"
      remarkPlugins={[remarkGfm]}
      urlTransform={markdownUrl}
      defer
      smooth
      components={{
        a: ({ href, children, ...props }) => {
          const location = href ? fileLocation(href) : undefined;
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
        },
      }}
    />
  );
}
function MessageImage({ image }: ImageMessagePartProps) {
  return <img className="message-image" src={image} alt="Attached image" />;
}
function useMessageCopy() {
  const host = useContext(ConversationContext);
  const content = useAuiState((state) => state.message.content);
  const [copied, setCopied] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; if (timer.current) clearTimeout(timer.current); };
  }, []);
  const copy = () => {
    void copyText(content.filter((part) => part.type === "text").map((part) => part.text).join("\n")).then(() => {
      if (!mounted.current) return;
      setCopied(true);
      if (timer.current) clearTimeout(timer.current);
      timer.current = setTimeout(() => setCopied(false), 3000);
    }).catch((error) => host?.store.setError(error));
  };
  return { copied, copy };
}
function UserMessage() {
  const host = useContext(ConversationContext);
  const messageId = useAuiState((s) => s.message.id);
  const { copied, copy } = useMessageCopy();
  const storedDate = host?.thread.messages.find((message) => message.id === messageId)?.createdAt;
  const date = storedDate ? new Date(storedDate instanceof Date ? storedDate.getTime() : String(storedDate)) : undefined;
  const validDate = date && Number.isFinite(date.getTime()) ? date : undefined;
  const hasDate = !!validDate;
  const threadId = host?.thread.id;
  const sessionId = host?.thread.sessionId;
  const store = host?.store;
  const connected = store?.getSnapshot().connected;
  useEffect(() => {
    if (!hasDate && threadId && sessionId && connected && store)
      void store.loadMessageTimes(threadId).catch(() => {});
  }, [hasDate, threadId, sessionId, connected, store]);
  return <MessagePrimitive.Root className="user-message group/user-message ml-auto mt-1 mb-8 flex w-fit max-w-[88%] min-w-0 flex-col items-end gap-1">
    <div data-slot="user-message-bubble" className="min-w-0 rounded-[17px] bg-[#242424] px-4 py-3 text-sm leading-5 whitespace-pre-wrap wrap-anywhere">
      <MessagePrimitive.Content components={{ Image: MessageImage }} />
    </div>
    <ActionBarPrimitive.Root className="pointer-events-none flex h-7 items-center gap-1 text-xs text-muted opacity-0 transition-opacity duration-150 group-hover/user-message:pointer-events-auto group-hover/user-message:opacity-100 group-focus-within/user-message:pointer-events-auto group-focus-within/user-message:opacity-100 motion-reduce:transition-none">
      {validDate && <time dateTime={validDate.toISOString()} title={validDate.toLocaleString()} className="mr-1 tabular-nums">{validDate.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", hour12: false })}</time>}
      <Tooltip><Button variant="ghost" size="icon-sm" className={`rounded-full ${copied ? "text-emerald-500 hover:text-emerald-500" : "text-muted hover:text-foreground"}`} aria-label={copied ? "Copied" : "Copy"} onClick={copy}>
        {copied ? <Check size={14} /> : <Copy size={14} />}
      </Button><TooltipContent>{copied ? "Copied" : "Copy"}</TooltipContent></Tooltip>
    </ActionBarPrimitive.Root>
  </MessagePrimitive.Root>;
}
function AssistantMessage() {
  const host = useContext(ConversationContext);
  const { copied, copy } = useMessageCopy();
  const messageId = useAuiState((s) => s.message.id);
  const [forking, setForking] = useState(false);
  const anchor = host?.thread.messageAnchors?.[messageId];
  return (
    <MessagePrimitive.Root className="assistant-message group/assistant-message">
      <MessagePrimitive.If last>
        <ToolActivity />
      </MessagePrimitive.If>
      <MessagePrimitive.Content components={{ Text: Markdown }} />
      <MessagePrimitive.If last={false}>
        <div className="message-divider" />
      </MessagePrimitive.If>
      <ActionBarPrimitive.Root className="pointer-events-none mt-3 flex h-7 items-center gap-1 opacity-0 transition-opacity duration-150 group-hover/assistant-message:pointer-events-auto group-hover/assistant-message:opacity-100 group-focus-within/assistant-message:pointer-events-auto group-focus-within/assistant-message:opacity-100 motion-reduce:transition-none" hideWhenRunning>
      <MessageActions copied={copied} reaction={null} regenerating={false} onCopy={copy} />
      {host?.thread.capabilities?.fork?.availability === "available" &&
        anchor &&
        host.thread.state === "idle" && (
          <Tooltip><Button
            variant="ghost"
            size="icon-sm"
            className="rounded-full text-muted"
            aria-label="Fork from here"
            disabled={forking}
            onClick={() => {
              setForking(true);
              void host.store
                .fork(host.thread.id, anchor, "after_assistant")
                .catch((e) => host.store.setError(e))
                .finally(() => setForking(false));
            }}
          >
            <GitFork size={14} />
          </Button><TooltipContent>Fork from here</TooltipContent></Tooltip>
        )}
      </ActionBarPrimitive.Root>
    </MessagePrimitive.Root>
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
  const upload = useRef<HTMLInputElement>(null);
  const [sending, setSending] = useState(false);
  const [caret, setCaret] = useState(0);
  const [previewAttachment, setPreviewAttachment] = useState<string>();
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
        runtime.thread.composer.setText(
          store.getSnapshot().threads.find((t) => t.id === thread.id)?.draft ??
            text,
        );
      }
    },
    [store, thread.id, thread.attachments],
  );
  const running = ["starting", "running", "cancelling", "needs_input"].includes(
    thread.state,
  );
  const runtime = useExternalStoreRuntime({
    messages: thread.messages,
    convertMessage: (message) => message,
    isRunning: running,
    onNew,
    onCancel: async () => {
      try {
        await store.cancel(thread.id);
      } catch (e) {
        store.setError(e);
      }
    },
  });
  useEffect(() => {
    runtime.thread.composer.setText(thread.draft ?? "");
  }, [thread.id]);
  useEffect(() => {
    const applyDraft = (event: Event) => {
      if ((event as CustomEvent).detail?.threadId === thread.id)
        runtime.thread.composer.setText(
          store.getSnapshot().threads.find((t) => t.id === thread.id)?.draft ??
            "",
        );
    };
    window.addEventListener("swarmie:composer-text", applyDraft);
    return () => window.removeEventListener("swarmie:composer-text", applyDraft);
  }, [thread.id]);
  const [checkingRecovery, setCheckingRecovery] = useState(false);
  const unresolvedSubmission = thread.submissionKeys?.some((entry) =>
    !entry.receipt || entry.receipt.recoveryRequired || !["accepted", "rejected"].includes(entry.receipt.delivery));
  const checkRecovery = async () => {
    setCheckingRecovery(true);
    try { await store.checkSubmissionRecovery(thread.id); }
    catch (error) { store.setError(error); }
    finally { setCheckingRecovery(false); }
  };
  const submitDuringTurn = async (delivery: "queue" | "steer") => {
    setSending(true);
    try {
      await store.send(
        thread.id,
        thread.draft ?? "",
        thread.attachments ?? [],
        delivery,
      );
      runtime.thread.composer.setText(
        store.getSnapshot().threads.find((t) => t.id === thread.id)?.draft ??
          "",
      );
    } catch (e) {
      store.setError(e);
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
      )
        throw new Error("Keep total attached content under 8 MB.");
      if ((thread.attachments?.length ?? 0) + next.length > 8)
        throw new Error("Attach up to eight files per message.");
      store.updateThread(thread.id, {
        attachments: [...(thread.attachments ?? []), ...next],
      });
    } catch (e) {
      store.setError(e);
    }
  };
  return (
    <AssistantRuntimeProvider runtime={runtime}>
      <ConversationContext.Provider value={{ thread, store }}>
        <ActivityContext.Provider value={{ tools: thread.tools, running }}>
          <ThreadPrimitive.Root className={`conversation ${thread.messages.length === 0 && !running ? "conversation-empty" : ""}`}>
            <ThreadPrimitive.Viewport
              className="conversation-scroll"
              autoScroll
            >
              <div className="conversation-width">
                <ThreadPrimitive.Empty>
                  <div className="welcome">
                    <span className="welcome-mark" aria-hidden="true"><Terminal size={30} /></span>
                    <h1>{projectName ? `What should we build in ${projectName}?` : "What should we build?"}</h1>
                  </div>
                </ThreadPrimitive.Empty>
                <ThreadPrimitive.Messages
                  components={{ UserMessage, AssistantMessage }}
                />
                {thread.reasoning && (
                  <Disclosure className="tool-activity">
                    <Disclosure.Summary>Thinking</Disclosure.Summary>
                    <p className="reasoning-text">{thread.reasoning}</p>
                  </Disclosure>
                )}
                {thread.plan != null && <Plan value={thread.plan} />}
                <OutputAssets store={store} files={thread.outputFiles ?? []} />
                {!!thread.subagents?.length && (
                  <Disclosure className="tool-activity" defaultOpen>
                    <Disclosure.Summary>Agents · {thread.subagents.length}</Disclosure.Summary>
                    {thread.subagents.map((agent, i) => (
                      <div
                        className="subagent-row"
                        key={String(agent._id ?? i)}
                      >
                        <strong>
                          {String(
                            agent.name ??
                              agent.role ??
                              agent.agentId ??
                              "Agent",
                          )}
                        </strong>
                        <span>
                          {String(agent.status ?? agent.state ?? "Working")}
                        </span>
                        {agent.progress != null && (
                          <p>{String(agent.progress)}</p>
                        )}
                      </div>
                    ))}
                  </Disclosure>
                )}
                {running && (
                  <div className="running-label">
                    <span className="status-dot working" />{" "}
                    {thread.state === "needs_input"
                      ? unresolvedSubmission ? "Checking delivery of your request" : "Waiting for your permission"
                      : "Working on your request…"}
                  </div>
                )}
                {unresolvedSubmission && (
                  <div role="status">
                    <p>We cannot confirm whether your request finished. Pending work is paused.</p>
                    <Button variant="ghost" disabled={checkingRecovery} onClick={() => void checkRecovery()}>
                      {checkingRecovery ? "Checking…" : "Check request status"}
                    </Button>
                  </div>
                )}
                {thread.error && (
                  <p className="inline-error" role="alert">
                    {errorMessage(thread.error, thread.harness)}
                  </p>
                )}
              </div>
            </ThreadPrimitive.Viewport>
            <div className="composer-dock">
              <div className="conversation-width">
                <ThreadPrimitive.ScrollToBottom asChild>
                  <Button
                    variant="ghost"
                    className="scroll-bottom icon-button"
                    aria-label="Jump to latest message"
                  >
                    <ChevronDown size={17} />
                  </Button>
                </ThreadPrimitive.ScrollToBottom>
                <Queue thread={thread} store={store} />
                {context && <div className="composer-context-bar">{context}</div>}
                <ComposerPrimitive.Root
                  className="composer"
                  onDragOver={(e) => e.preventDefault()}
                  onDrop={(e) => {
                    e.preventDefault();
                    void attach(Array.from(e.dataTransfer.files));
                  }}
                >
                  <ComposerContext
                    thread={thread}
                    store={store}
                    caret={caret}
                    setText={(text) => runtime.thread.composer.setText(text)}
                  />
                  <SlashCommands
                    thread={thread}
                    store={store}
                    setText={(text) => runtime.thread.composer.setText(text)}
                  />
                  {!!thread.attachments?.length && (
                    <div className="attachments">
                      {thread.attachments.map((file) => (
                        <div className="attachment" key={file.id}>
                          {file.image && (
                            <img
                              src={`data:${file.mime};base64,${file.data}`}
                              alt={file.name}
                            />
                          )}
                          <Button
                            variant="ghost"
                            className="small-button"
                            onClick={() =>
                              setPreviewAttachment(
                                previewAttachment === file.id
                                  ? undefined
                                  : file.id,
                              )
                            }
                          >
                            {file.name}
                          </Button>
                          <Button
                            variant="ghost"
                            className="icon-button"
                            aria-label={`Remove ${file.name}`}
                            onClick={() =>
                              store.updateThread(thread.id, {
                                attachments: thread.attachments?.filter(
                                  (a) => a.id !== file.id,
                                ),
                              })
                            }
                          >
                            <X size={13} />
                          </Button>
                        </div>
                      ))}
                    </div>
                  )}
                  {thread.attachments
                    ?.filter(
                      (a) =>
                        a.id === previewAttachment && (a.image || a.binary),
                    )
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
                      void attach(Array.from(e.target.files ?? []));
                      e.target.value = "";
                    }}
                  />
                  <ComposerPrimitive.Input
                    className="composer-input"
                    placeholder="Ask anything, or describe a task…"
                    aria-label="Message"
                    disabled={!ready}
                    rows={2}
                    onChange={(e) => {
                      setCaret(e.target.selectionStart);
                      store.updateThread(thread.id, { draft: e.target.value });
                    }}
                    onSelect={(e) => setCaret(e.currentTarget.selectionStart)}
                    onPaste={(e) => {
                      if (e.clipboardData.files.length) {
                        e.preventDefault();
                        void attach(Array.from(e.clipboardData.files));
                      }
                    }}
                    autoFocus
                  />
                  <div className="composer-toolbar">
                    <Button
                      variant="ghost"
                      size="icon-sm"
                      className="icon-button rounded-full"
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
                    <VoiceInput key={thread.id} disabled={!ready} onText={(text) => {
                      const draft = store.getSnapshot().threads.find((item) => item.id === thread.id)?.draft ?? "";
                      runtime.thread.composer.setText(`${draft}${draft && !/\s$/.test(draft) ? " " : ""}${text}`);
                    }} onError={(error) => store.setError(error)} />
                    <ThreadPrimitive.If running={false}>
                      <ComposerPrimitive.Send asChild>
                        <Button
                          size="icon-sm"
                          className="send-button rounded-full"
                          aria-label="Send message"
                          type="submit"
                          disabled={!ready}
                        >
                          <ArrowUp size={18} />
                        </Button>
                      </ComposerPrimitive.Send>
                    </ThreadPrimitive.If>
                    <ThreadPrimitive.If running>
                      <Button
                        variant="ghost"
                        size="sm"
                        className="small-button"
                        disabled={
                          sending ||
                          !(thread.draft?.trim() || thread.attachments?.length)
                        }
                        onClick={() => void submitDuringTurn("queue")}
                      >
                        <ListPlus size={14} />
                        Queue
                      </Button>
                      {thread.capabilities?.steering?.availability === "available" && (
                        <Button
                          variant="ghost"
                          size="sm"
                          className="small-button"
                          disabled={sending || !thread.draft?.trim()}
                          onClick={() => void submitDuringTurn("steer")}
                        >
                          <CornerUpLeft size={14} />
                          Steer
                        </Button>
                      )}
                      <ComposerPrimitive.Cancel asChild>
                        <Button
                          size="icon-sm"
                          className="send-button stop-button rounded-full"
                          aria-label="Stop generation"
                        >
                          <Square size={13} fill="currentColor" />
                        </Button>
                      </ComposerPrimitive.Cancel>
                    </ThreadPrimitive.If>
                    </div>
                  </div>
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
  return (
    <Button
      variant="ghost"
      className="small-button"
      onClick={async () => {
        await copyText(text);
        setCopied(true);
        setTimeout(() => setCopied(false), 1800);
      }}
    >
      {copied ? <Check size={14} /> : <Copy size={14} />}{" "}
      {copied ? "Copied" : "Copy patch"}
    </Button>
  );
}
