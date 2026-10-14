import { useEffect, useId, useRef, useState } from "react";
import type { ComponentPropsWithRef, ReactNode } from "react";

import { Avatar } from "../../atoms/avatar";
import { Badge } from "../../atoms/badge";
import { Button } from "../../atoms/button";
import { Icon } from "../../atoms/icon";
import { Textarea } from "../../atoms/textarea";
import { TooltipContent, TooltipProvider, TooltipRoot, TooltipTrigger } from "../../atoms/tooltip";
import { cn } from "../../classes";
import { AlertContent, AlertDescription, AlertRoot, AlertTitle } from "../alert";
import { AssistantPanel, PanelHeading, TooltipIconButton } from "./shared";
import { ThinkingState } from "./thinking-state";

export type MessageRootProps = ComponentPropsWithRef<"article"> & {
  speaker: string;
  authorKind: "human" | "assistant" | "system";
  time?: ReactNode;
  arrival?: "instant" | "animated";
};

/** Open message layout preserves Swarmie's session measure. The author remains explicit. */
export function MessageRoot({
  speaker,
  authorKind,
  time,
  arrival = "instant",
  className,
  children,
  ...props
}: MessageRootProps) {
  return (
    <article
      {...props}
      data-arrival={arrival}
      className={cn("assistant-message group/message max-w-prose space-y-2", className)}
    >
      <header className="flex flex-wrap items-center gap-2 text-ui">
        <span className="font-medium">{speaker}</span>
        {authorKind === "assistant" ? <Badge tone="agent">Agent</Badge> : null}
        <span className="ml-auto text-xs text-text-muted">{time}</span>
      </header>
      <div
        className={cn(
          "min-w-0 text-body leading-body break-words",
          authorKind === "human" && "rounded-panel bg-surface-muted p-3",
        )}
      >
        {children}
      </div>
    </article>
  );
}

export function MessagePair({ human, assistant }: { human: ReactNode; assistant: ReactNode }) {
  return (
    <div className="flex flex-col gap-4">
      {human}
      {assistant}
    </div>
  );
}

export function MessageBranches({
  current,
  count,
  label,
  previousLabel,
  nextLabel,
  onChange,
}: {
  current: number;
  count: number;
  label: string;
  previousLabel: string;
  nextLabel: string;
  onChange: (index: number) => void;
}) {
  return (
    <nav aria-label={label} className="inline-flex items-center gap-1">
      <TooltipIconButton
        label={previousLabel}
        icon="back"
        disabled={count < 2}
        onClick={() => {
          onChange((current - 1 + count) % count);
        }}
      />
      <span className="min-w-14 text-center text-ui tabular-nums">
        {count === 0 ? 0 : current + 1} / {count}
      </span>
      <TooltipIconButton
        label={nextLabel}
        icon="arrow"
        disabled={count < 2}
        onClick={() => {
          onChange((current + 1) % count);
        }}
      />
    </nav>
  );
}

export function MessageError({
  title,
  message,
  retryLabel,
  onRetry,
  retrying = false,
  retryingLabel,
}: {
  title: string;
  message: string;
  retryLabel: string;
  onRetry: () => void;
  retrying?: boolean;
  retryingLabel?: string;
}) {
  if (retrying) {
    return (
      <output
        key="retrying"
        aria-live="polite"
        className="flex items-center gap-2 text-ui motion-safe:animate-assistant-receipt"
      >
        <Icon name="loading" className="size-3.5 motion-safe:animate-assistant-spin" />
        <ThinkingState>{retryingLabel ?? retryLabel}</ThinkingState>
      </output>
    );
  }
  return (
    <AlertRoot key="error" tone="error" className="motion-safe:animate-assistant-receipt">
      <AlertContent>
        <AlertTitle>{title}</AlertTitle>
        <AlertDescription>{message}</AlertDescription>
        <Button className="mt-3" variant="secondary" onClick={onRetry}>
          {retryLabel}
        </Button>
      </AlertContent>
    </AlertRoot>
  );
}

export type QueuedMessage = { id: string; text: string };

export function MessageQueue({
  messages,
  label,
  cancelLabel,
  onCancel,
  runningMessage,
}: {
  runningMessage?: string;
  messages: readonly QueuedMessage[];
  label: string;
  cancelLabel: string;
  onCancel: (id: string) => void;
}) {
  return (
    <AssistantPanel aria-label={label}>
      <PanelHeading>
        {label} <Badge>{messages.length}</Badge>
      </PanelHeading>
      {runningMessage === undefined ? null : (
        <output className="mb-3 flex items-center gap-2 text-ui">
          <span
            aria-hidden="true"
            className="size-2 rounded-full bg-primary motion-safe:animate-pulse"
          />
          {runningMessage}
        </output>
      )}
      <ul className="space-y-2">
        {messages.map((message) => (
          <li
            key={message.id}
            className="flex items-center gap-3 motion-safe:animate-assistant-receipt"
          >
            <Icon name="clock" />
            <p className="min-w-0 flex-1 text-ui break-words">{message.text}</p>
            <TooltipIconButton
              label={`${cancelLabel}: ${message.text}`}
              icon="close"
              onClick={() => {
                onCancel(message.id);
              }}
            />
          </li>
        ))}
      </ul>
    </AssistantPanel>
  );
}

export function EditMessage({
  label,
  value,
  warning,
  saveLabel,
  cancelLabel,
  onChange,
  onSave,
  onCancel,
  editing = true,
  onEdit,
  editLabel,
}: {
  editing?: boolean;
  onEdit?: () => void;
  editLabel?: string;
  label: string;
  value: string;
  warning: string;
  saveLabel: string;
  cancelLabel: string;
  onChange: (value: string) => void;
  onSave: () => void;
  onCancel: () => void;
}) {
  if (!editing) {
    return (
      <div className="space-y-2">
        <p className="text-body leading-body">{value}</p>
        {onEdit && editLabel !== undefined ? (
          <Button variant="ghost" onClick={onEdit}>
            {editLabel}
          </Button>
        ) : null}
      </div>
    );
  }
  return (
    <div className="space-y-3">
      <label className="flex flex-col gap-2 text-ui font-medium">
        {label}
        <Textarea
          value={value}
          onChange={(event) => {
            onChange(event.target.value);
          }}
        />
      </label>
      <p className="text-ui text-text-muted">{warning}</p>
      <div className="flex flex-wrap gap-2">
        <Button onClick={onSave} disabled={!value.trim()}>
          {saveLabel}
        </Button>
        <Button variant="ghost" onClick={onCancel}>
          {cancelLabel}
        </Button>
      </div>
    </div>
  );
}

export function FeedbackForm({
  label,
  reasons,
  selected,
  note,
  noteLabel,
  submitLabel,
  onToggle,
  onNoteChange,
  onSubmit,
  receipt,
}: {
  label: string;
  reasons: readonly string[];
  selected: readonly string[];
  note: string;
  noteLabel: string;
  submitLabel: string;
  onToggle?: (reason: string) => void;
  onNoteChange: (value: string) => void;
  onSubmit: () => void;
  receipt?: string;
}) {
  return (
    <div>
      <output aria-live="polite" className="flex items-center gap-2 text-ui text-success">
        {receipt === undefined ? null : (
          <span key={receipt} className="motion-safe:animate-assistant-receipt">
            {receipt}
          </span>
        )}
      </output>
      {receipt === undefined ? (
        <AssistantPanel>
          <fieldset className="space-y-3">
            <legend className="font-medium">{label}</legend>
            <div className="flex flex-wrap gap-2">
              {reasons.map((reason) => (
                <Button
                  key={reason}
                  variant="secondary"
                  aria-pressed={selected.includes(reason)}
                  onClick={() => {
                    onToggle?.(reason);
                  }}
                >
                  {reason}
                </Button>
              ))}
            </div>
            <label className="flex flex-col gap-2 text-ui">
              {noteLabel}
              <Textarea
                value={note}
                onChange={(event) => {
                  onNoteChange(event.target.value);
                }}
              />
            </label>
            <Button onClick={onSubmit} disabled={selected.length === 0 && !note.trim()}>
              {submitLabel}
            </Button>
          </fieldset>
        </AssistantPanel>
      ) : null}
    </div>
  );
}

export function StoppedRun({
  label,
  continueLabel,
  onContinue,
  onDiscard,
  discardLabel,
  children,
}: {
  label: string;
  continueLabel: string;
  onContinue: () => void;
  onDiscard?: () => void;
  discardLabel?: string;
  children: ReactNode;
}) {
  return (
    <div className="space-y-3">
      {children}
      <div className="flex flex-wrap items-center gap-3">
        <Badge>{label}</Badge>
        <Button variant="secondary" onClick={onContinue}>
          <Icon name="history" />
          {continueLabel}
        </Button>
        {onDiscard && discardLabel !== undefined ? (
          <Button variant="ghost" onClick={onDiscard}>
            {discardLabel}
          </Button>
        ) : null}
      </div>
    </div>
  );
}

export function MessageTimestamp({
  dateTime,
  children,
  reveal = "always",
}: {
  dateTime: string;
  reveal?: "always" | "hover";
  children: ReactNode;
}) {
  return (
    <time
      dateTime={dateTime}
      className={cn(
        "text-xs tabular-nums transition-colors motion-reduce:transition-none",
        reveal === "hover"
          ? "text-transparent group-focus-within/message:text-text-muted group-hover/message:text-text-muted pointer-coarse:text-text-muted"
          : "text-text-muted",
      )}
    >
      {children}
    </time>
  );
}

export function DaySeparator({ children }: { children: ReactNode }) {
  return (
    <div className="flex items-center justify-center py-3 text-xs text-text-muted">{children}</div>
  );
}

export function SpeakerIdentity({
  name,
  initials,
  detail,
  speakerKind = "human",
}: {
  speakerKind?: "human" | "agent" | "subagent" | "tool";
  name: string;
  initials: string;
  detail?: string;
}) {
  return (
    <div className="flex items-center gap-3">
      {speakerKind === "human" ? (
        <Avatar kind="person" label={name} initials={initials} />
      ) : (
        <Icon name={speakerKind === "tool" ? "task" : "agent"} />
      )}
      <div className="min-w-0">
        <p className="text-ui font-medium">{name}</p>
        {detail === undefined ? null : <p className="text-xs text-text-muted">{detail}</p>}
      </div>
    </div>
  );
}

export function ConfidenceMarker({
  label,
  basis,
  explanation,
  children,
}: {
  label: string;
  basis: "sourced" | "inferred" | "uncertain";
  explanation?: string;
  children: ReactNode;
}) {
  return (
    <span className="inline-flex flex-wrap items-center gap-2">
      {children}
      <TooltipProvider>
        <TooltipRoot>
          <TooltipTrigger asChild>
            <Button variant="ghost" size="sm">
              <Badge tone={basis === "sourced" ? "success" : "attention"}>{label}</Badge>
            </Button>
          </TooltipTrigger>
          <TooltipContent>{explanation ?? label}</TooltipContent>
        </TooltipRoot>
      </TooltipProvider>
    </span>
  );
}

export function QuoteReply({
  author,
  children,
  actionLabel,
  onQuote,
}: {
  author: string;
  children: ReactNode;
  actionLabel: string;
  onQuote: () => void;
}) {
  return (
    <AssistantPanel>
      <blockquote className="space-y-2">
        <p className="text-content leading-content">{children}</p>
        <footer className="text-xs text-text-muted">{author}</footer>
      </blockquote>
      <Button className="mt-3" variant="secondary" onClick={onQuote}>
        <Icon name="chat" />
        {actionLabel}
      </Button>
    </AssistantPanel>
  );
}

export function DirectiveText({
  parts,
}: {
  parts: readonly (
    | { id: string; kind: "text"; text: string }
    | { id: string; kind: "mention"; label: string; onClick: () => void }
    | { id: string; kind: "directive"; category: string; label: string; onClick?: () => void }
  )[];
}) {
  return (
    <p className="text-body leading-body">
      {parts.map((part) =>
        part.kind === "text" ? (
          <span key={part.id}>{part.text}</span>
        ) : (
          <Button key={part.id} variant="link" size="inline" onClick={part.onClick}>
            {part.kind === "mention" ? "@" : `${part.category}: `}
            {part.label}
          </Button>
        ),
      )}
    </p>
  );
}

/** Selection lifecycle remains within one message; the host owns the resulting composer quote. */
export function QuoteSelection({
  children,
  label,
  onQuote,
}: {
  children: ReactNode;
  label: string;
  onQuote: (text: string) => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [selection, setSelection] = useState("");
  useEffect(() => {
    const update = () => {
      const range = window.getSelection();
      if (
        !range ||
        range.isCollapsed ||
        ref.current?.contains(range.anchorNode) !== true ||
        !ref.current.contains(range.focusNode)
      ) {
        setSelection("");
        return;
      }
      setSelection(range.toString().trim());
    };
    document.addEventListener("selectionchange", update);
    return () => {
      document.removeEventListener("selectionchange", update);
    };
  }, []);
  return (
    <div ref={ref} className="space-y-2">
      {children}
      {selection.length > 0 ? (
        <Button
          variant="secondary"
          onMouseDown={(event) => {
            event.preventDefault();
          }}
          onClick={() => {
            onQuote(selection);
            setSelection("");
            window.getSelection()?.removeAllRanges();
          }}
        >
          {label}
        </Button>
      ) : null}
    </div>
  );
}

export function ComposerQuote({
  text,
  label,
  removeLabel,
  onRemove,
}: {
  text: string;
  label: string;
  removeLabel: string;
  onRemove: () => void;
}) {
  const id = useId();
  return (
    <aside aria-labelledby={id} className="flex items-start gap-2 rounded-panel bg-surface p-3">
      <div className="min-w-0 flex-1">
        <p id={id} className="text-xs text-text-muted">
          {label}
        </p>
        <blockquote className="text-ui">{text}</blockquote>
      </div>
      <TooltipIconButton label={removeLabel} icon="close" onClick={onRemove} />
    </aside>
  );
}
