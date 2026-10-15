import type { ReactNode } from "react";
import { useRef, useState } from "react";

import { Button } from "../../../atoms/button";
import { ClaudeLogo, GeminiLogo, OpenAILogo } from "../../../atoms/model-logos";
import { ApprovalCard } from "../agents";
import { FileAttachment } from "../attachment";
import type { AssistantElementSlugFor } from "../catalog";
import {
  ComposerInput,
  ComposerRoot,
  ComposerTriggerPicker,
  applyComposerMention,
  matchComposerMention,
  ContextBreakdown,
  ContextDisplay,
  ModelSelector,
} from "../composer";
import { GenerativeUI } from "../generative";
import { ImageGallery, Sources } from "../knowledge";
import { ComposerQuote, DirectiveText, MessageError, QuoteSelection } from "../messages";
import { MessageTiming } from "../observability";
import { MarkdownText, MermaidDiagram, ShikiHighlighter, SyntaxHighlighter } from "../renderers";
import { CodeSurface, TooltipIconButton } from "../shared";
import { ActivityGraph } from "../structured";
import { ThinkingReasoning } from "../thinking-reasoning";
import {
  AssistantModal,
  AssistantSidebar,
  ConversationMap,
  FollowUpSuggestions,
  ThreadList,
} from "../thread";
import { ServerConfigForm, ServerPanel, ToolCall, ToolGroup } from "../tools";
import { VoiceOrb } from "../voice";
import { SessionExample } from "./composer-thread";
import { demoImage, ExampleReceipt, models, sources, threads, useExampleAction } from "./fixtures";
import { ThreadWorkspaceExample } from "./thread-workspace";

type ConnectedRendererSlug = AssistantElementSlugFor<"AUI connected" | "Renderers" | "Primitives">;

const reviewFlowDiagram =
  "flowchart LR\n  Request --> Inspect\n  Inspect --> Review\n  Review --> Decision";

function ConnectedQuoteExample() {
  const [quote, setQuote] = useState("");
  const [draft, setDraft] = useState("");
  const { receipt, record } = useExampleAction();
  return (
    <div className="space-y-3">
      <QuoteSelection label="Quote in a reply" onQuote={setQuote}>
        <p className="text-ui">
          The application owns execution, while the components own presentation.
        </p>
      </QuoteSelection>
      <ComposerRoot
        value={draft}
        onValueChange={setDraft}
        onSubmit={(value) => {
          record(quote ? `Reply sent with quote: ${value}` : `Reply sent: ${value}`);
          setDraft("");
          setQuote("");
        }}
      >
        {quote ? (
          <ComposerQuote
            text={quote}
            label="Replying to Swarmie"
            removeLabel="Remove quote"
            onRemove={() => {
              setQuote("");
            }}
          />
        ) : null}
        <ComposerInput label="Reply" placeholder="Write a reply…" />
      </ComposerRoot>
      <ExampleReceipt receipt={receipt} />
    </div>
  );
}

function ConnectedTriggerExample() {
  const [draft, setDraft] = useState("Ask @");
  const input = useRef<HTMLTextAreaElement>(null);
  const query = matchComposerMention(draft);
  const { receipt, record } = useExampleAction();
  return (
    <ComposerRoot value={draft} onValueChange={setDraft} onSubmit={record}>
      <ComposerInput ref={input} label="Message" />
      <ComposerTriggerPicker
        trigger="@"
        query={query ?? ""}
        open={query !== null}
        label="Mention a participant"
        items={[
          {
            id: "swarmie",
            label: "Swarmie",
            description: "Workspace assistant",
            insertion: "Swarmie",
          },
          {
            id: "alex",
            label: "Alex",
            description: "Design collaborator",
            insertion: "Alex",
            initials: "AL",
          },
        ]}
        onInsert={(name) => {
          setDraft((value) => applyComposerMention(value, name));
          input.current?.focus();
        }}
        onSelect={(id) => {
          record(`Inserted @${id}`);
        }}
      />
      <ExampleReceipt receipt={receipt} />
    </ComposerRoot>
  );
}

export function ConnectedRendererExample({ slug }: { slug: ConnectedRendererSlug }) {
  const { receipt, record } = useExampleAction();
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState("design");
  const [model, setModel] = useState("balanced");
  const [sidebarOpen, setSidebarOpen] = useState(true);
  const [hasAttachment, setHasAttachment] = useState(true);
  const [muted, setMuted] = useState(false);
  const sidebar = (
    <ThreadList
      label="Sessions"
      items={threads}
      activeId={active}
      onSelect={setActive}
      actionLabel="Session actions"
      onAction={(id) => {
        record(`Actions for ${id}`);
      }}
    />
  );
  const tool = (
    <ToolCall
      name="Inspect component contracts"
      activeLabel="Inspecting component contracts"
      state="completed"
      statusLabel="Complete"
      requestLabel="Request"
      resultLabel="Result"
      request={<CodeSurface label="Request">{'{"path":"packages/ui"}'}</CodeSurface>}
      result={<p className="text-ui">Found the shared component exports and stories.</p>}
    />
  );
  const renders: Record<ConnectedRendererSlug, ReactNode> = {
    thread: <ThreadWorkspaceExample presentation="thread" />,
    "assistant-modal": (
      <>
        <Button
          onClick={() => {
            setOpen(true);
          }}
        >
          Open assistant
        </Button>
        <AssistantModal
          open={open}
          onOpenChange={setOpen}
          title="Swarmie assistant"
          closeLabel="Close assistant"
        >
          <SessionExample compact />
        </AssistantModal>
      </>
    ),
    "assistant-sidebar": sidebarOpen ? (
      <AssistantSidebar
        title="Contextual assistant"
        closeLabel="Close sidebar"
        onClose={() => {
          setSidebarOpen(false);
          record("Sidebar closed");
        }}
      >
        <SessionExample compact />
      </AssistantSidebar>
    ) : (
      <Button
        onClick={() => {
          setSidebarOpen(true);
        }}
      >
        Reopen sidebar
      </Button>
    ),
    "thread-list": sidebar,
    "thread-list-sidebar": <ThreadWorkspaceExample presentation="embedded" />,
    orb: (
      <div className="flex flex-wrap items-center gap-6">
        <VoiceOrb state={muted ? "muted" : "speaking"} label={muted ? "Muted" : "Speaking"} />
        <Button
          variant="secondary"
          aria-pressed={muted}
          onClick={() => {
            setMuted((value) => !value);
          }}
        >
          Mute microphone
        </Button>
      </div>
    ),
    reasoning: (
      <ThinkingReasoning
        phase="completed"
        thinkingLabel="Reviewing the shared UI contracts"
        completedLabel="Reviewed the shared UI contracts"
        toggleLabel="Toggle the contract review"
        sentences={[
          {
            id: "ownership",
            text: "The components should consume Swarmie data and delegate actions to their host.",
          },
          {
            id: "composition",
            text: "Named parts let each application compose its own session without repeating the primitives.",
          },
        ]}
      />
    ),
    "message-timing": (
      <MessageTiming
        label="Message generation timing"
        firstToken="420 ms"
        total="3.8 s"
        speed="62 tokens/s"
      />
    ),
    "conversation-map": (
      <ConversationMap
        label="Session position"
        activeId={active}
        items={[
          { id: "design", label: "Design question", speaker: "Alex", visible: true },
          { id: "reply", label: "Component response", speaker: "Swarmie", visible: true },
          { id: "review", label: "Review request", speaker: "Alex", visible: false },
        ]}
        onSelect={setActive}
      />
    ),
    "context-display": (
      <ContextDisplay
        label="Model context"
        used={42_000}
        maximum={128_000}
        detail={
          <ContextBreakdown
            label="Context details"
            maximum={128_000}
            segments={[
              { id: "prompt", label: "Prompt", tokens: 8000, detail: "Workspace instructions" },
              {
                id: "files",
                label: "Files",
                tokens: 34_000,
                detail: "Shared documents and conversation",
              },
            ]}
          />
        }
      />
    ),
    "mcp-config": <ConnectorConfigurationExample />,
    attachment: hasAttachment ? (
      <FileAttachment
        name="component-preview.png"
        detail="Image · 4.2 KB · Ready"
        image={{ src: demoImage, alt: "Illustrative shared document" }}
        previewLabel="Preview attached image"
        closeLabel="Close attached image"
        removeLabel="Remove"
        onRemove={() => {
          setHasAttachment(false);
          record("Attachment removed");
        }}
      />
    ) : null,
    "follow-up-suggestions": (
      <FollowUpSuggestions
        label="Suggested next steps"
        suggestions={[
          { id: "compare", text: "Compare the approaches" },
          { id: "review", text: "Review the keyboard flow" },
          { id: "sources", text: "Show the sources" },
        ]}
        onSelect={(id) => {
          record(`Selected ${id}`);
        }}
      />
    ),
    "tool-fallback": <ConnectedToolExample />,
    "tool-group": (
      <ToolGroup label="2 tools completed">
        {tool}
        <ToolCall
          name="Read the frontend guide"
          activeLabel="Reading the frontend guide"
          state="completed"
          statusLabel="Complete"
          requestLabel="Request"
          resultLabel="Result"
          request={<p className="text-ui">docs/agents/frontend.md</p>}
          result={<p className="text-ui">Use shared primitives and controlled contracts.</p>}
        />
      </ToolGroup>
    ),
    quote: <ConnectedQuoteExample />,
    sources: <Sources label="Response sources" sources={sources} />,
    image: (
      <ImageGallery
        label="Message image"
        images={[
          {
            id: "document",
            src: demoImage,
            alt: "Illustrative shared document",
            caption: "Document illustration",
          },
        ]}
        onSelect={(id) => {
          record(`Previewing ${id.alt}`);
        }}
      />
    ),
    file: (
      <FileAttachment
        name="component-contract.md"
        detail="Markdown · 4.2 KB"
        source="message"
        href="data:text/plain,Shared%20component%20contract%20demo"
        downloadLabel="Download"
      />
    ),
    "model-selector": (
      <ModelSelector label="Model" models={models} value={model} onChange={setModel} />
    ),
    "composer-trigger-popover": <ConnectedTriggerExample />,
    "directive-text": (
      <DirectiveText
        parts={[
          { id: "start", kind: "text", text: "Ask " },
          {
            id: "mention",
            kind: "mention",
            label: "Swarmie",
            onClick: () => {
              record("Swarmie profile opened");
            },
          },
          { id: "end", kind: "text", text: " to inspect the next step." },
        ]}
      />
    ),
    "markdown-text": (
      <MarkdownText
        text={
          '## A clear next step\n\nUse **shared primitives** and keep actions explicit.\n\n- Inspect the contract\n- Compare the tradeoffs\n- Review the result\n\n| Component | Owner |\n| --- | --- |\n| Message | Design system |\n| Transport | Application |\n\n```ts\nconst decision = "review";\n```\n\n[Read the design guide](https://github.com/syntropika/swarmie/blob/main/docs/client-runtime/DESIGN.md)'
        }
      />
    ),
    "syntax-highlighter": (
      <SyntaxHighlighter
        language="javascript"
        label="JavaScript source"
        source={
          '// Shared presentation\nconst message = "Hello, workspace";\nfunction renderMessage(text) {\n  return text.trim();\n}'
        }
      />
    ),
    "shiki-highlighter": (
      <ShikiHighlighter
        language="typescript"
        label="TypeScript source"
        source={
          "type Message = { speaker: string; text: string };\nexport function title(message: Message): string {\n  return message.text.trim();\n}"
        }
      />
    ),
    "mermaid-diagram": (
      <MermaidDiagram
        source={reviewFlowDiagram}
        label="Request to decision flow"
        pendingLabel="Rendering the diagram"
        invalidLabel="The diagram is still incomplete"
      />
    ),
    "generative-ui": (
      <GenerativeUI
        node={{
          id: "proposal",
          kind: "card",
          title: "Proposed next step",
          children: [
            { id: "summary", kind: "text", text: "Review the component contracts with the team." },
            {
              id: "facts",
              kind: "facts",
              items: [
                { label: "Owner", value: "Design team" },
                { label: "Scope", value: "Shared UI" },
              ],
            },
            {
              id: "actions",
              kind: "actions",
              actions: [
                { id: "review", label: "Review proposal" },
                { id: "later", label: "Later", variant: "secondary" },
              ],
            },
          ],
        }}
        onFieldChange={(id, value) => {
          record(`${id}: ${value}`);
        }}
        onAction={record}
      />
    ),
    "tooltip-icon-button": (
      <TooltipIconButton
        label="Open session details"
        icon="panel"
        onClick={() => {
          record("Session details opened");
        }}
      />
    ),
    logos: (
      <ul className="flex flex-wrap gap-8">
        {[
          { id: "openai", label: "OpenAI", Logo: OpenAILogo },
          { id: "anthropic", label: "Anthropic", Logo: ClaudeLogo },
          { id: "google", label: "Google", Logo: GeminiLogo },
        ].map(({ id, label, Logo }) => (
          <li key={id} className="flex items-center gap-3">
            <Logo className="size-7 text-ink" />
            <span className="text-ui">{label}</span>
          </li>
        ))}
      </ul>
    ),
    "heat-graph": (
      <ActivityGraph
        label="Workspace activity · April–September"
        days={Array.from({ length: 182 }, (_, index) => ({
          date: new Date(Date.UTC(2026, 3, 1 + index)).toISOString().slice(0, 10),
          value: (index * 11) % 5,
          label: `Demo day ${index + 1}: ${(index * 11) % 5} events`,
        }))}
        legend="Mon–Sun · Light to dark: fewer to more events"
      />
    ),
  };
  const content = renders[slug];
  return (
    <>
      {content}
      <ExampleReceipt receipt={receipt} />
    </>
  );
}

type ToolDemoState =
  | { kind: "running"; approved: boolean; retrying: boolean }
  | { kind: "waiting" }
  | { kind: "completed"; approved: boolean; result: readonly string[] }
  | { kind: "failed" }
  | { kind: "cancelled" };

/** Local event controls stand in for host runtime events; no external tool is executed. */
function ConnectedToolExample() {
  const [state, setState] = useState<ToolDemoState>({
    kind: "running",
    approved: false,
    retrying: false,
  });
  const [expanded, setExpanded] = useState(true);
  const statusLabels = {
    running: "Running",
    waiting: "Waiting for approval",
    completed: "Complete",
    failed: "Failed",
    cancelled: "Denied",
  };
  const run = () => {
    setExpanded(true);
    setState({ kind: "running", approved: false, retrying: false });
  };
  let interrupt: ReactNode;
  if (state.kind === "waiting") {
    interrupt = (
      <ApprovalCard
        state="request"
        title="Allow the demo inspection?"
        subtitle="Read shared component contracts"
        description="This fixture only reads the in-memory example inventory."
        allowOnceLabel="Allow inspection once"
        alwaysAllowLabel="Always allow"
        denyLabel="Deny inspection"
        statusLabel="Waiting for a decision"
        onAllowOnce={() => {
          setState({ kind: "running", approved: true, retrying: false });
        }}
        onDeny={() => {
          setState({ kind: "cancelled" });
        }}
      />
    );
  }
  if ((state.kind === "running" || state.kind === "completed") && state.approved) {
    interrupt = <output className="block text-ui">Allowed this inspection once</output>;
  }
  if (state.kind === "cancelled") {
    interrupt = (
      <output className="block text-ui">Inspection denied; no result was produced.</output>
    );
  }
  const retrying = state.kind === "running" && state.retrying;
  return (
    <div className="space-y-3">
      <p className="text-xs text-text-muted">
        Demo runtime events update the same controlled tool call.
      </p>
      <ToolCall
        name="Inspect component contracts"
        activeLabel="Inspecting component contracts"
        query="packages/ui"
        state={state.kind}
        statusLabel={statusLabels[state.kind]}
        open={expanded}
        onOpenChange={setExpanded}
        requestLabel="Arguments"
        resultLabel="Result"
        request={
          <CodeSurface label="Tool arguments">
            {JSON.stringify({ path: "packages/ui", include: ["exports", "stories"] }, null, 2)}
          </CodeSurface>
        }
        result={
          state.kind === "completed" ? (
            <CodeSurface label="Tool result">
              {JSON.stringify({ files: state.result, count: state.result.length }, null, 2)}
            </CodeSurface>
          ) : undefined
        }
        interrupt={interrupt}
        error={
          state.kind === "failed" || retrying ? (
            <MessageError
              title="Inspection failed"
              message="The demo inventory was temporarily unavailable."
              retryLabel="Retry inspection"
              retrying={retrying}
              retryingLabel="Retrying inspection"
              onRetry={() => {
                setState({ kind: "running", approved: false, retrying: true });
              }}
            />
          ) : undefined
        }
      />
      <div className="flex flex-wrap gap-2">
        {state.kind === "running" ? (
          <>
            <Button
              onClick={() => {
                setState({
                  kind: "completed",
                  approved: state.approved,
                  result: ["src/index.ts", "src/molecules/assistant/renderers.stories.tsx"],
                });
              }}
            >
              Return demo result
            </Button>
            <Button
              variant="secondary"
              onClick={() => {
                setExpanded(true);
                setState({ kind: "waiting" });
              }}
            >
              Require approval
            </Button>
            <Button
              variant="ghost"
              onClick={() => {
                setState({ kind: "failed" });
              }}
            >
              Simulate tool failure
            </Button>
          </>
        ) : (
          <Button variant="secondary" onClick={run}>
            Restart demo inspection
          </Button>
        )}
      </div>
    </div>
  );
}

type ConnectorDemoState =
  | { kind: "empty" }
  | { kind: "running" | "completed" | "failed" | "idle"; url: string };

/** Connection completion/failure are explicit fixture events, not fabricated network results. */
function ConnectorConfigurationExample() {
  const [open, setOpen] = useState(false);
  const [url, setUrl] = useState("https://example.com/mcp");
  const [auth, setAuth] = useState("");
  const [connection, setConnection] = useState<ConnectorDemoState>({ kind: "empty" });
  const labels = {
    running: "Connecting demo server",
    completed: "Connected in demo",
    failed: "Connection failed",
    idle: "Disconnected",
  };
  const pending = connection.kind === "running";
  const reconnect = () => {
    if (connection.kind !== "empty") {
      setConnection({ kind: "running", url: connection.url });
    }
  };
  return (
    <>
      <Button
        onClick={() => {
          setOpen(true);
        }}
      >
        Configure connectors
      </Button>
      <AssistantModal
        open={open}
        onOpenChange={setOpen}
        title="Workspace connectors"
        closeLabel="Close connectors"
      >
        <p className="mb-3 text-ui text-text-muted">
          Local configuration demo. Connection events below do not contact the supplied server.
        </p>
        <ServerConfigForm
          url={url}
          auth={auth}
          urlLabel="Connector URL"
          authLabel="Authorization token"
          submitLabel="Save demo connector"
          pending={pending}
          onUrlChange={setUrl}
          onAuthChange={setAuth}
          onSubmit={(configuration) => {
            setConnection({ kind: "running", url: configuration.url });
            setAuth("");
          }}
        />
        {connection.kind === "empty" ? (
          <p className="text-ui">No custom demo connectors.</p>
        ) : (
          <ServerPanel
            label="Configured sources"
            servers={[
              {
                id: "custom",
                name: connection.url,
                state: connection.kind,
                statusLabel: labels[connection.kind],
                transport: "Streamable HTTP",
                tools:
                  connection.kind === "completed"
                    ? ["Search shared documents", "Read shared document"]
                    : [],
                ...(connection.kind === "failed"
                  ? { error: "The demo server rejected the connection." }
                  : {}),
                ...(connection.kind === "completed"
                  ? {
                      actionLabel: "Disconnect demo",
                      onAction: () => {
                        setConnection({ kind: "idle", url: connection.url });
                      },
                    }
                  : {}),
                ...(connection.kind === "failed" || connection.kind === "idle"
                  ? { actionLabel: "Retry demo connection", onAction: reconnect }
                  : {}),
                removeLabel: "Remove demo connector",
                onRemove: () => {
                  setConnection({ kind: "empty" });
                },
              },
            ]}
          />
        )}
        {connection.kind === "running" ? (
          <div className="mt-3 flex flex-wrap gap-2">
            <Button
              onClick={() => {
                setConnection({ kind: "completed", url: connection.url });
              }}
            >
              Finish demo connection
            </Button>
            <Button
              variant="secondary"
              onClick={() => {
                setConnection({ kind: "failed", url: connection.url });
              }}
            >
              Simulate connection failure
            </Button>
          </div>
        ) : null}
      </AssistantModal>
    </>
  );
}
