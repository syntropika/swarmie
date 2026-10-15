import type { ReactNode } from "react";
import { useEffect, useRef, useState } from "react";

import { Button } from "../../../atoms/button";
import { Textarea } from "../../../atoms/textarea";
import type { AssistantElementSlugFor } from "../catalog";
import {
  applyComposerMention,
  matchComposerMention,
  CommandPalette,
  ComposerAttachButton,
  ComposerAttachments,
  ComposerInput,
  ComposerRail,
  ComposerRoot,
  ComposerSubmit,
  ComposerTriggerPicker,
  ContextBreakdown,
  ContextDisplay,
  Dictation,
  DraftRestore,
  ModelDetails,
  ModelSelector,
  PromptLibrary,
} from "../composer";
import type { ComposerAttachment } from "../composer";
import { Sources } from "../knowledge";
import { MessageRoot } from "../messages";
import { CostMeter, QuotaBanner, TraceWaterfall } from "../observability";
import { ReasoningEffort } from "../reasoning-effort";
import { MarkdownText } from "../renderers";
import { ThinkingState } from "../thinking-state";
import {
  AssistantLauncher,
  AssistantModal,
  AssistantOnboarding,
  AssistantSettings,
  CanvasSplit,
  ConnectionState,
  ConversationSearch,
  EmptyThread,
  MobileComposer,
  ScrollAnchor,
  SharedConversation,
  ThreadComposer,
  ThreadRoot,
  ThreadSearch,
  ThreadViewport,
} from "../thread";
import { ReadAloud, VoiceConversation } from "../voice";
import { demoImage, ExampleReceipt, models, sources, threads, useExampleAction } from "./fixtures";

type ComposerThreadSlug = AssistantElementSlugFor<
  "Observability" | "Composer" | "Voice" | "Thread"
>;

type DemoMessage = { id: string; speaker: string; role: "human" | "assistant"; text: string };

export function SessionExample({ compact = false }: { compact?: boolean }) {
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const [value, setValue] = useState("");
  const [messages, setMessages] = useState<DemoMessage[]>([
    {
      id: "initial",
      speaker: "Swarmie",
      role: "assistant" as const,
      text: "What would you like to work on? I can help compare approaches, inspect the details, or prepare a clear next step.",
    },
  ]);
  const [model, setModel] = useState("balanced");
  const [busy, setBusy] = useState(false);
  const [files, setFiles] = useState<readonly ComposerAttachment[]>([]);
  const commands = [
    { id: "review", label: "review", description: "Review a proposal" },
    { id: "research", label: "research", description: "Investigate a question" },
  ];
  const people = [
    { id: "swarmie", label: "Swarmie", description: "Workspace assistant", initials: "SW" },
    { id: "alex", label: "Alex", description: "Design collaborator", initials: "AM" },
  ];
  const send = (text: string) => {
    setMessages((items) => [
      ...items,
      { id: crypto.randomUUID(), speaker: "You", role: "human", text },
      {
        id: crypto.randomUUID(),
        speaker: "Swarmie",
        role: "assistant",
        text: `I’ve received “${text}”. This is a local Storybook conversation; your workspace adapter supplies real responses.`,
      },
    ]);
    setValue("");
    setFiles([]);
    setBusy(false);
  };
  const composer = (
    <ComposerRoot value={value} onValueChange={setValue} onSubmit={send} busy={busy}>
      <ComposerAttachments
        files={files}
        previewLabel={(name) => `Preview ${name}`}
        closeLabel="Close preview"
        removeLabel="Remove attachment"
        onRemove={(id) => {
          setFiles((items) => items.filter((item) => item.id !== id));
        }}
      />
      <ComposerInput ref={inputRef} label="Message" placeholder="Ask, plan, or share context…" />
      {/^\/\S*$/u.test(value) ? (
        <ComposerTriggerPicker
          trigger="/"
          query={value.slice(1)}
          items={commands}
          label="Slash commands"
          onSelect={(id) => {
            setValue(`/${id} `);
            inputRef.current?.focus();
          }}
        />
      ) : null}
      {matchComposerMention(value) === null ? null : (
        <ComposerTriggerPicker
          trigger="@"
          query={matchComposerMention(value) ?? ""}
          items={people}
          label="Mention a participant"
          onSelect={(id) => {
            setValue(applyComposerMention(value, id));
            inputRef.current?.focus();
          }}
        />
      )}
      <ComposerRail>
        <ComposerAttachButton
          label="Attach"
          onFiles={(selected) => {
            setFiles((items) => [
              ...items,
              ...selected.map((file) => ({
                id: crypto.randomUUID(),
                name: file.name,
                detail: `${file.size} bytes · Staged locally`,
              })),
            ]);
          }}
        />
        <ModelSelector label="Model" models={models} value={model} onChange={setModel} />
        <ComposerSubmit
          label="Send"
          stopLabel="Stop"
          onStop={() => {
            setBusy(false);
          }}
        />
      </ComposerRail>
    </ComposerRoot>
  );
  return (
    <ThreadRoot className={compact ? "h-96" : "h-[36rem]"}>
      <ThreadViewport label="Session messages">
        {messages.map((message) => (
          <MessageRoot
            key={message.id}
            speaker={message.speaker}
            authorKind={message.role}
            arrival={message.id === "initial" ? "instant" : "animated"}
          >
            <MarkdownText text={message.text} />
          </MessageRoot>
        ))}
        {busy ? (
          <output aria-live="polite">
            <ThinkingState>Preparing a demo response</ThinkingState>
          </output>
        ) : null}
      </ThreadViewport>
      <ThreadComposer>{composer}</ThreadComposer>
    </ThreadRoot>
  );
}

export function ComposerThreadExample({ slug }: { slug: ComposerThreadSlug }) {
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const { receipt, record } = useExampleAction();
  const initialValues = new Map([
    ["composer-slash-commands", "/re"],
    ["composer-mentions", "@sw"],
  ]);
  const [value, setValue] = useState(
    initialValues.get(slug) ?? "Help me review the component contracts.",
  );
  const [model, setModel] = useState("balanced");
  const [activeId, setActiveId] = useState("design");
  const [temperature, setTemperature] = useState(0.7);
  const [capabilities, setCapabilities] = useState([
    { id: "search", label: "Search sources", detail: "Find supporting evidence", enabled: true },
    { id: "tools", label: "Use tools", detail: "Allow tool requests", enabled: false },
  ]);
  const [draft, setDraft] = useState<string | null>("Review the open design questions.");
  const [keyboardOpen, setKeyboardOpen] = useState(false);
  const [hitIndex, setHitIndex] = useState(0);
  const [step, setStep] = useState(0);
  const [open, setOpen] = useState(false);
  const [voice, setVoice] = useState(false);
  const [muted, setMuted] = useState(false);
  const [voiceTick, setVoiceTick] = useState(0);
  useEffect(() => {
    if (!voice) {
      return undefined;
    }
    const timer = setInterval(() => {
      setVoiceTick((current) => current + 1);
    }, 100);
    return () => {
      clearInterval(timer);
    };
  }, [voice]);
  const [speed, setSpeed] = useState("1");
  const [files, setFiles] = useState<readonly ComposerAttachment[]>([
    {
      id: "preview",
      name: "component-preview.png",
      detail: "Image · 4.2 KB · Ready",
      image: { src: demoImage, alt: "Illustrative shared document" },
      status: "ready",
    },
    { id: "guide", name: "component-guide.md", detail: "4.2 KB · Ready", status: "ready" },
    {
      id: "notes",
      name: "review-notes.txt",
      detail: "2.1 KB · Uploading",
      progress: 64,
      status: "uploading",
    },
    {
      id: "failed",
      name: "old-contract.pdf",
      detail: "Upload failed. Try adding the file again.",
      status: "failed",
    },
  ]);
  const composer = (
    <ComposerRoot
      value={value}
      onValueChange={setValue}
      onSubmit={(text) => {
        record(`Sent: ${text}`);
        setValue("");
      }}
    >
      {slug === "composer-attachments" ? (
        <ComposerAttachments
          files={files}
          previewLabel={(name) => `Preview ${name}`}
          closeLabel="Close preview"
          removeLabel="Remove"
          onRemove={(id) => {
            setFiles((items) => items.filter((item) => item.id !== id));
          }}
        />
      ) : null}
      {slug === "composer-voice" && voice ? null : (
        <ComposerInput
          ref={inputRef}
          label="Message"
          placeholder="Ask, plan, or share context…"
          onFocus={() => {
            setKeyboardOpen(true);
          }}
          onBlur={() => {
            setKeyboardOpen(false);
          }}
        />
      )}
      {slug === "composer-slash-commands" && /^\/\S*$/u.test(value) ? (
        <ComposerTriggerPicker
          trigger="/"
          query={value.slice(1)}
          label="Slash commands"
          items={[
            { id: "review", label: "review", description: "Review a proposal" },
            { id: "research", label: "research", description: "Find evidence" },
          ]}
          onSelect={(id) => {
            setValue(`/${id} `);
            inputRef.current?.focus();
          }}
        />
      ) : null}
      {slug === "composer-mentions" && typeof matchComposerMention(value) === "string" ? (
        <ComposerTriggerPicker
          trigger="@"
          query={matchComposerMention(value) ?? ""}
          label="Mentions"
          items={[
            { id: "swarmie", label: "Swarmie", description: "Workspace assistant", initials: "SW" },
            { id: "alex", label: "Alex", description: "Design collaborator", initials: "AM" },
          ]}
          onSelect={(id) => {
            setValue(applyComposerMention(value, id));
            inputRef.current?.focus();
          }}
        />
      ) : null}
      {slug === "composer-voice" ? (
        <Dictation
          label="Start dictation"
          stopLabel="Stop dictation"
          active={voice}
          tick={voiceTick}
          transcript={voice ? "Please review the component contracts." : "Dictation is off"}
          onToggle={() => {
            setVoice((current) => !current);
            setValue("Please review the component contracts.");
          }}
        />
      ) : null}
      <ComposerRail>
        {slug === "composer-attachments" ? (
          <ComposerAttachButton
            label="Add files"
            onFiles={(selected) => {
              setFiles((items) => [
                ...items,
                ...selected.map((file) => ({
                  id: crypto.randomUUID(),
                  name: file.name,
                  detail: `${file.size} bytes · Staged locally`,
                })),
              ]);
            }}
          />
        ) : null}
        <ModelSelector label="Model" models={models} value={model} onChange={setModel} />
        <ComposerSubmit
          label="Send"
          stopLabel="Stop"
          onStop={() => {
            record("Stopped");
          }}
        />
      </ComposerRail>
    </ComposerRoot>
  );
  const suggestions = [
    { id: "review", title: "Review a proposal", detail: "Compare options and tradeoffs" },
    { id: "plan", title: "Plan the next step", detail: "Turn context into clear actions" },
    {
      id: "research",
      title: "Investigate a question",
      detail: "Find the evidence behind an answer",
    },
  ];
  const renders: Record<ComposerThreadSlug, ReactNode> = {
    "trace-waterfall": (
      <TraceWaterfall
        label="Run timing"
        duration={1800}
        spans={[
          {
            id: "run",
            label: "Run",
            start: 0,
            duration: 1800,
            depth: 0,
            detail: "1.8 seconds total",
            state: "running",
            statusLabel: "Running",
          },
          {
            id: "search",
            label: "Search",
            start: 100,
            duration: 640,
            depth: 1,
            detail: "640 ms",
            state: "completed",
            statusLabel: "Completed",
          },
          {
            id: "read",
            label: "Read",
            start: 750,
            duration: 420,
            depth: 1,
            detail: "420 ms",
            state: "failed",
            statusLabel: "Failed",
          },
          {
            id: "reply",
            label: "Respond",
            start: 1180,
            duration: 620,
            depth: 1,
            detail: "620 ms",
            state: "running",
            statusLabel: "Streaming",
          },
        ]}
      />
    ),
    "cost-meter": (
      <CostMeter
        label="Session cost"
        budget={0.1}
        sessionAmount={0.048}
        costs={[
          {
            id: "balanced",
            model: "Demo balanced model",
            amount: 0.0142,
            tokens: "8,400 tokens",
            inputTokens: 7000,
            outputTokens: 1400,
          },
          {
            id: "fast",
            model: "Demo fast model",
            amount: 0.0021,
            tokens: "1,800 tokens",
            inputTokens: 1400,
            outputTokens: 400,
          },
        ]}
      />
    ),
    "quota-banner": (
      <QuotaBanner
        title="12 of 100 runs remaining"
        remaining={12}
        usageLabel="Runs used"
        unit="runs"
        maximum={100}
        resetLabel="Your demo quota resets tomorrow at 09:00."
        actionLabel="View usage"
        onAction={() => {
          record("Usage details opened");
        }}
      />
    ),
    composer,
    "composer-slash-commands": composer,
    "composer-mentions": composer,
    "composer-attachments": composer,
    "composer-model-picker": (
      <div className="space-y-3">
        <ModelSelector label="Choose a model" models={models} value={model} onChange={setModel} />
        <ModelDetails model={models.find((item) => item.id === model) ?? models[0]} />
      </div>
    ),
    "composer-voice": composer,
    "composer-context": (
      <ContextDisplay
        label="Session context"
        presentation="popup"
        segments={[
          { id: "system", label: "System", tokens: 8000, detail: "Session instructions" },
          { id: "tools", label: "Tools", tokens: 18_000, detail: "Available tools" },
          { id: "messages", label: "Messages", tokens: 16_000, detail: "Conversation turns" },
        ]}
        used={42_000}
        maximum={128_000}
        detail={
          <p className="mt-3 text-ui text-text-muted">
            42,000 of 128,000 tokens · Files and conversation
          </p>
        }
      />
    ),
    "draft-restore": (
      <div className="space-y-3">
        {draft === null ? null : (
          <DraftRestore
            label="Your unsent draft is available"
            preview={draft}
            savedAt="Saved 2 minutes ago"
            restoreLabel="Restore draft"
            discardLabel="Discard"
            onRestore={() => {
              setValue(draft);
              setDraft(null);
              inputRef.current?.focus();
              record(`Draft restored: ${draft}`);
            }}
            onDiscard={() => {
              setDraft(null);
              record("Draft discarded");
            }}
          />
        )}
        {composer}
      </div>
    ),
    "context-breakdown": (
      <ContextBreakdown
        label="Context usage"
        maximum={128_000}
        segments={[
          {
            id: "system",
            label: "System instructions",
            tokens: 8000,
            detail: "Workspace and session instructions",
          },
          {
            id: "files",
            label: "Files",
            tokens: 18_000,
            detail: "Shared documentation and source files",
          },
          {
            id: "messages",
            label: "Conversation",
            tokens: 16_000,
            detail: "Messages in this session",
          },
        ]}
      />
    ),
    "prompt-library": (
      <div className="space-y-3">
        <PromptLibrary
          label="Saved prompts"
          searchLabel="Search prompts"
          insertLabel="Insert prompt"
          prompts={[
            {
              id: "review",
              title: "Review a component",
              text: "Review {{component}} for keyboard access and clear state transitions.",
              variables: [{ name: "component", label: "Component name" }],
            },
            {
              id: "summarize",
              title: "Summarize a decision",
              text: "Summarize the evidence, tradeoffs, and next step.",
              variables: [],
            },
          ]}
          onInsert={(id, values) => {
            const prompt =
              id === "review"
                ? `Review ${values.component} for keyboard access and clear state transitions.`
                : "Summarize the evidence, tradeoffs, and next step.";
            setValue(prompt);
            inputRef.current?.focus();
            record(`Inserted ${id}: ${Object.values(values).join(", ")}`);
          }}
        />
        {composer}
      </div>
    ),
    "command-palette": (
      <CommandPalette
        label="Workspace commands"
        placeholder="Find an action…"
        emptyLabel="No matching commands"
        items={[
          { id: "new", label: "New session", group: "Workspace" },
          { id: "search", label: "Search sessions", group: "Workspace" },
          { id: "settings", label: "Appearance settings", group: "Account" },
        ]}
        onSelect={(id) => {
          record(`Selected ${id}`);
        }}
      />
    ),
    "voice-conversation": (
      <VoiceConversation
        state={muted ? "muted" : "listening"}
        amplitude={0.6}
        label={muted ? "Microphone muted" : "Listening · Demo call"}
        transcript={
          <p>
            Alex: Let’s review the next step.
            <br />
            Swarmie: The component contracts are ready.
          </p>
        }
        muted={muted}
        onMuteChange={setMuted}
        muteLabel="Mute microphone"
        endLabel="End call"
        onEnd={() => {
          record("Demo call ended");
        }}
      />
    ),
    "read-aloud": (
      <ReadAloud
        label="Read this response aloud"
        words={[
          { id: "one", text: "The" },
          { id: "two", text: "components" },
          { id: "three", text: "share" },
          { id: "four", text: "one" },
          { id: "five", text: "clear" },
          { id: "six", text: "contract." },
        ]}
        elapsed={voice ? "0:04" : "0:00"}
        duration="0:12"
        activeIndex={voice ? 2 : -1}
        playing={voice}
        speed={speed}
        playLabel="Play"
        pauseLabel="Pause"
        speedLabel="Playback speed"
        onPlayChange={setVoice}
        onSpeedChange={setSpeed}
      />
    ),
    "chat-panel": <SessionExample />,
    "empty-state": (
      <EmptyThread
        title="What would you like to work on?"
        description="Bring a question, a draft, or a decision. We’ll find a useful next step together."
        suggestions={suggestions}
        onSelect={(id) => {
          setValue(suggestions.find((item) => item.id === id)?.title ?? "");
        }}
      >
        {composer}
      </EmptyThread>
    ),
    "scroll-anchor": (
      <ScrollAnchor
        label="Back to the latest message"
        onClick={() => {
          record("Jumped to the latest message");
        }}
      />
    ),
    "canvas-split": (
      <CanvasSplit
        label="Component contract notes"
        closeLabel="Close document"
        onClose={() => {
          record("Canvas closed");
        }}
        conversation={<SessionExample compact />}
        canvas={
          <MarkdownText text="## Shared contract\n\n- Data comes from the application.\n- Actions are explicit callbacks.\n- Components share Swarmie tokens." />
        }
      />
    ),
    "connection-state": (
      <ExampleConnection
        restored={receipt.length > 0}
        onRetry={() => {
          record("Connection restored");
        }}
      />
    ),
    "shared-conversation": (
      <SharedConversation
        title="Shared design review"
        attribution="Read-only demo shared by Alex"
        forkLabel="Continue in your workspace"
        onFork={() => {
          record("Conversation copied to your workspace");
        }}
      >
        <MessageRoot speaker="Swarmie" authorKind="assistant">
          The components use the existing Swarmie design system.
          <Sources label="References" sources={sources} />
        </MessageRoot>
      </SharedConversation>
    ),
    "conversation-search": (
      <ConversationSearch
        label="Find in this session"
        activeIndex={hitIndex}
        onActiveIndexChange={setHitIndex}
        value={value}
        onChange={setValue}
        hits={
          value.trim().length > 0
            ? [
                {
                  id: "contract",
                  author: "Swarmie",
                  before: "The shared ",
                  match: value,
                  after: " belongs to the application.",
                },
              ]
            : []
        }
        emptyLabel="Enter a search term"
        onSelect={(id) => {
          record(`Jumped to ${id}`);
        }}
      />
    ),
    "thread-search": (
      <ThreadSearch
        label="Sessions"
        searchLabel="Search sessions"
        items={threads}
        activeId={activeId}
        onSelect={setActiveId}
      />
    ),
    "launcher-bubble": (
      <AssistantLauncher
        label="Open assistant"
        open={open}
        onOpenChange={setOpen}
        unread={2}
        greeting="How can we help?"
        prompts={["Review the design", "Plan a session"]}
        onPick={(prompt) => {
          setValue(prompt);
          record(prompt);
        }}
        startLabel="Start a conversation"
        onStart={() => {
          record("Conversation started");
        }}
      >
        <SessionExample compact />
      </AssistantLauncher>
    ),
    "settings-panel": (
      <AssistantSettings
        title="Assistant settings"
        temperature={temperature}
        temperatureLabel="Temperature"
        onTemperatureChange={setTemperature}
        capabilities={capabilities}
        onCapabilityChange={(id, enabled) => {
          setCapabilities((current) =>
            current.map((item) => (item.id === id ? { ...item, enabled } : item)),
          );
        }}
      >
        <ModelSelector label="Model" models={models} value={model} onChange={setModel} />
        <label htmlFor="demo-system-instructions" className="flex flex-col gap-2 text-ui">
          System instructions
          <Textarea
            id="demo-system-instructions"
            value={value}
            onChange={(event) => {
              setValue(event.target.value);
            }}
          />
        </label>
        <ReasoningEffort
          label="Reasoning effort"
          spent={4200}
          usageLabel="Reasoning budget used"
          value={speed}
          options={[
            { value: "1", label: "Standard", budget: 8000 },
            { value: "2", label: "Extended", budget: 32_000 },
          ]}
          onChange={setSpeed}
        />
      </AssistantSettings>
    ),
    onboarding: (
      <AssistantOnboarding
        index={step}
        onChange={setStep}
        nextLabel="Next"
        backLabel="Back"
        finishLabel="Start a session"
        skipLabel="Skip onboarding"
        onSkip={() => {
          record("Onboarding skipped");
        }}
        onFinish={() => {
          record("Onboarding completed");
        }}
        steps={[
          {
            id: "question",
            title: "Start with a question",
            description: "Share the context behind the work.",
          },
          {
            id: "context",
            title: "Choose the shared context",
            description: "Attach the sources everyone can inspect.",
          },
          {
            id: "decide",
            title: "Decide the next step",
            description: "Review the proposal and keep responsibility clear.",
          },
        ]}
      />
    ),
    "mobile-composer": (
      <MobileComposer
        label="Mobile message input"
        keyboardOpen={keyboardOpen}
        actions={
          <Button
            variant="ghost"
            onClick={() => {
              record("Attachment picker opened");
            }}
          >
            Attach a file
          </Button>
        }
      >
        {composer}
      </MobileComposer>
    ),
  };
  const content = renders[slug];
  return (
    <>
      {content}
      <ExampleReceipt receipt={receipt} />
      <AssistantModal
        open={isDetailsOpen(open, slug)}
        onOpenChange={setOpen}
        title="Session details"
        closeLabel="Close details"
      >
        <p>Demo session metadata</p>
      </AssistantModal>
    </>
  );
}

function isDetailsOpen(open: boolean, slug: ComposerThreadSlug) {
  return open && slug !== "launcher-bubble";
}

function ExampleConnection({ restored, onRetry }: { restored: boolean; onRetry: () => void }) {
  return (
    <ConnectionState
      state={restored ? "resumed" : "offline"}
      label={restored ? "Stream recovered · 128 tokens" : "Connection lost"}
      detail="The run continues on the server while the connection recovers."
      retryLabel="Reconnect now"
      onRetry={onRetry}
    />
  );
}
