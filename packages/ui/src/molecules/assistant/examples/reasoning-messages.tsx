import type { ReactNode } from "react";
import { useState } from "react";

import { Button } from "../../../atoms/button";
import type { AssistantElementSlugFor } from "../catalog";
import { RegenerateMenu } from "../composer";
import { ConversationArrival } from "../conversation-motion";
import { MessageActions } from "../message-actions";
import type { MessageVote } from "../message-actions";
import {
  ConfidenceMarker,
  DaySeparator,
  EditMessage,
  FeedbackForm,
  MessageBranches,
  MessageError,
  MessagePair,
  MessageQueue,
  MessageRoot,
  MessageTimestamp,
  SpeakerIdentity,
  StoppedRun,
} from "../messages";
import { GuardrailNotice, TypingIndicator } from "../reasoning";
import { ReasoningEffort } from "../reasoning-effort";
import { MarkdownText } from "../renderers";
import { StreamingText } from "../streaming-text";
import { ThinkingState } from "../thinking-state";
import { ExampleReceipt, models, useExampleAction } from "./fixtures";
import { ExampleAssistantLoader, ExampleStreamingText } from "./streaming";

type ReasoningMessageSlug = AssistantElementSlugFor<"Reasoning" | "Messages">;

const datedGroups = [
  {
    day: "2026-09-30",
    label: "Yesterday · September 30",
    messages: [
      {
        id: "question",
        speaker: "Alex",
        kind: "human",
        dateTime: "2026-09-30T15:00:00Z",
        time: "16:00",
        text: "Can we review the design system tomorrow?",
      },
      {
        id: "plan",
        speaker: "Swarmie",
        kind: "assistant",
        dateTime: "2026-09-30T15:01:00Z",
        time: "16:01",
        text: "The component contracts are ready for review.",
      },
    ],
  },
  {
    day: "2026-10-01",
    label: "Today · October 1",
    messages: [
      {
        id: "review",
        speaker: "Alex",
        kind: "human",
        dateTime: "2026-10-01T11:00:00Z",
        time: "12:00",
        text: "Let’s review the design system.",
      },
    ],
  },
] satisfies readonly {
  day: string;
  label: string;
  messages: readonly {
    id: string;
    speaker: string;
    kind: "human" | "assistant";
    dateTime: string;
    time: string;
    text: string;
  }[];
}[];

export function ReasoningMessageExample({ slug }: { slug: ReasoningMessageSlug }) {
  const [value, setValue] = useState("Compare the two approaches and explain the tradeoffs.");
  const [effort, setEffort] = useState("medium");
  const [vote, setVote] = useState<MessageVote>(null);
  const [reaction, setReaction] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const [regenerateOpen, setRegenerateOpen] = useState(false);
  const [editing, setEditing] = useState(false);
  const [retrying, setRetrying] = useState(false);
  const [stopped, setStopped] = useState(true);
  const [model, setModel] = useState("balanced");
  const [branch, setBranch] = useState(0);
  const [selected, setSelected] = useState<readonly string[]>([]);
  const [queue, setQueue] = useState([
    { id: "test", text: "Run the interaction checks." },
    { id: "docs", text: "Update the component documentation." },
  ]);
  const { receipt, record } = useExampleAction();
  const renders: Record<ReasoningMessageSlug, ReactNode> = {
    "loading-state": <ExampleAssistantLoader label="Preparing the next step" />,
    "thinking-indicator": (
      <output aria-live="polite">
        <ThinkingState activity elapsed="4 seconds">
          Checking the component contracts
        </ThinkingState>
      </output>
    ),
    "streaming-text": (
      <ExampleStreamingText text="The components share Swarmie’s typography and color tokens. Each action has an explicit owner." />
    ),
    "typing-indicator": <TypingIndicator label="Swarmie is composing a response" />,
    "reasoning-effort": (
      <ReasoningEffort
        label="Reasoning effort"
        value={effort}
        onChange={setEffort}
        options={[
          { value: "low", label: "Low", budget: 1000 },
          { value: "medium", label: "Medium", budget: 8000 },
          { value: "high", label: "High", budget: 32_000 },
        ]}
        spent={4200}
        usageLabel="Reasoning budget used"
      />
    ),
    "guardrail-notice": (
      <GuardrailNotice
        title="This source is outside the session’s access"
        policy="workspace-access"
        alternatives={["Use the shared documentation", "Ask the source owner for access"]}
        onPick={(choice) => {
          record(choice);
        }}
        alternative="Use the shared documentation"
        onAlternative={() => {
          record("Use shared documentation");
        }}
      >
        Choose a source that everyone in this session can access.
      </GuardrailNotice>
    ),
    "message-pair": (
      <MessagePair
        human={
          <MessageRoot speaker="Alex" authorKind="human">
            How should we organize these components?
          </MessageRoot>
        }
        assistant={
          <MessageRoot speaker="Swarmie" authorKind="assistant" className="group">
            <ExampleStreamingText text="Group them by what people need to understand or act on. Keep the visual primitives shared." />
            <div className="flex gap-2 opacity-0 transition-opacity duration-200 group-focus-within:opacity-100 group-hover:opacity-100 motion-reduce:transition-none pointer-coarse:opacity-100">
              <Button
                variant="ghost"
                onClick={() => {
                  setCopied(true);
                }}
              >
                {copied ? "Copied" : "Copy response"}
              </Button>
              <Button
                variant="ghost"
                disabled={retrying}
                onClick={() => {
                  setRetrying(true);
                }}
              >
                {retrying ? "Regenerating" : "Regenerate response"}
              </Button>
              <output className="sr-only">{copied ? "Copied" : ""}</output>
            </div>
          </MessageRoot>
        }
      />
    ),
    "message-branches": (
      <div className="space-y-4">
        <ConversationArrival key={branch}>
          <MarkdownText
            text={
              [
                "Use controlled components for clear ownership.",
                "Use named parts for flexible composition.",
                "Start with shared primitives and add specific contracts.",
              ][branch] ?? ""
            }
          />
        </ConversationArrival>
        <MessageBranches
          current={branch}
          count={3}
          label="Response versions"
          previousLabel="Previous response"
          nextLabel="Next response"
          onChange={setBranch}
        />
      </div>
    ),
    "message-actions": (
      <MessageActions
        label="Response actions"
        labels={{
          good: "Helpful response",
          bad: "Unhelpful response",
          copy: "Copy response",
          copied: "Copied",
          react: "Add reaction",
          reaction: (emoji) => `Reaction ${emoji}`,
          reply: "Reply to response",
          more: "More response actions",
          aloud: "Read aloud",
          fork: "Fork chat",
          report: "Report an issue",
          search: "Search emoji",
          empty: "No emoji found",
          back: "Back to quick reactions",
        }}
        text="The components share Swarmie’s typography and color tokens. Each action has an explicit owner."
        vote={vote}
        onVote={(nextVote) => {
          setVote(nextVote);
          record(nextVote === null ? "Feedback cleared" : `Feedback: ${nextVote}`);
        }}
        reaction={reaction}
        onReact={setReaction}
        copied={copied}
        onCopy={() => {
          setCopied(true);
          record("Copied response");
        }}
        onRegenerate={() => {
          setRetrying(true);
        }}
        regenerating={retrying}
        onReply={() => {
          record("Reply requested");
        }}
        onAloud={() => {
          record("Read aloud requested");
        }}
        onFork={() => {
          record("Fork requested");
        }}
        onReport={() => {
          record("Feedback opened");
        }}
      />
    ),
    "error-state": (
      <MessageError
        title="The response could not finish"
        message="The connection was interrupted. Your message is still here."
        retryLabel="Try again"
        retrying={retrying}
        retryingLabel="Trying again"
        onRetry={() => {
          setRetrying(true);
          record("Retry requested");
        }}
      />
    ),
    "message-queue": (
      <MessageQueue
        label="Queued messages"
        runningMessage="Reviewing the component contracts"
        messages={queue}
        cancelLabel="Remove"
        onCancel={(id) => {
          setQueue((items) => items.filter((item) => item.id !== id));
          record("Queued message removed");
        }}
      />
    ),
    "edit-message": (
      <EditMessage
        label="Edit your message"
        editing={editing}
        editLabel="Edit message"
        onEdit={() => {
          setEditing(true);
        }}
        value={value}
        onChange={setValue}
        warning="Saving replaces this message and the two responses after it."
        saveLabel="Save edit"
        cancelLabel="Cancel"
        onSave={() => {
          setEditing(false);
          record(`Edit saved: ${value}`);
        }}
        onCancel={() => {
          setEditing(false);
          record("Edit cancelled");
        }}
      />
    ),
    "feedback-dialog": (
      <FeedbackForm
        label="What could be better?"
        reasons={["Incorrect", "Missing context", "Hard to follow"]}
        selected={selected}
        note={value}
        noteLabel="Additional details"
        submitLabel="Send feedback"
        onToggle={(reason) => {
          setSelected((items) =>
            items.includes(reason) ? items.filter((item) => item !== reason) : [...items, reason],
          );
        }}
        onNoteChange={setValue}
        onSubmit={() => {
          record("Feedback sent");
        }}
        {...(receipt.length > 0 ? { receipt } : {})}
      />
    ),
    "stopped-run": stopped ? (
      <StoppedRun
        label="Stopped by you"
        continueLabel="Continue response"
        discardLabel="Discard response"
        onDiscard={() => {
          setStopped(false);
          record("Response discarded");
        }}
        onContinue={() => {
          record("Response continued");
        }}
      >
        <StreamingText
          segments={[
            { text: "The first approach reduces the number of dependencies. The second approach…" },
          ]}
          count={100}
          streaming={false}
        />
      </StoppedRun>
    ) : (
      <output>Response discarded</output>
    ),
    "day-separator": (
      <div className="space-y-4">
        {datedGroups.map((group) => (
          <section key={group.day} aria-label={group.label}>
            <DaySeparator>{group.label}</DaySeparator>
            {group.messages.map((message) => (
              <MessageRoot
                key={message.id}
                speaker={message.speaker}
                authorKind={message.kind}
                time={
                  <MessageTimestamp dateTime={message.dateTime} reveal="hover">
                    {message.time}
                  </MessageTimestamp>
                }
              >
                <p>{message.text}</p>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => {
                    record(`Replying to ${message.speaker}`);
                  }}
                >
                  Reply to {message.speaker}
                </Button>
              </MessageRoot>
            ))}
          </section>
        ))}
      </div>
    ),
    "speaker-identity": (
      <div className="space-y-3">
        <SpeakerIdentity name="Alex Morgan" initials="AM" detail="Design collaborator" />
        <SpeakerIdentity name="Swarmie" initials="SW" speakerKind="agent" detail="Assistant" />
        <SpeakerIdentity
          name="Research worker"
          initials="RW"
          speakerKind="subagent"
          detail="Delegated evidence review"
        />
        <SpeakerIdentity name="Search" initials="S" speakerKind="tool" detail="Source lookup" />
      </div>
    ),
    "regenerate-menu": (
      <RegenerateMenu
        label="Regenerate with"
        currentLabel="Current"
        models={models}
        currentId={model}
        open={regenerateOpen}
        onOpenChange={setRegenerateOpen}
        onPick={(id) => {
          setModel(id);
          record(`Regenerate with ${id}`);
        }}
      />
    ),
    "confidence-marker": (
      <div className="space-y-3">
        <ConfidenceMarker
          basis="sourced"
          label="Supported by a source"
          explanation="The shared font tokens declare DM Sans."
        >
          The library uses DM Sans.
        </ConfidenceMarker>
        <ConfidenceMarker
          basis="inferred"
          label="Inference"
          explanation="This follows from the frequent-work layout, but has not been tested with users."
        >
          A compact layout may suit frequent work.
        </ConfidenceMarker>
        <ConfidenceMarker
          basis="uncertain"
          label="Needs verification"
          explanation="No completion-time measurement has been collected."
        >
          The new flow may reduce completion time.
        </ConfidenceMarker>
      </div>
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
