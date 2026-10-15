import { useLayoutEffect, useRef, useState } from "react";
import type { ComponentPropsWithRef, ReactNode } from "react";

import { Avatar } from "../../atoms/avatar";
import { Badge } from "../../atoms/badge";
import { Button } from "../../atoms/button";
import { Icon } from "../../atoms/icon";
import { SelectionControl } from "../../atoms/selection-control";
import { cn } from "../../classes";
import {
  AssistantDisclosure,
  AssistantPanel,
  AssistantProgress,
  ExecutionStatus,
  Facts,
  PanelHeading,
} from "./shared";
import type { ExecutionState } from "./shared";

export type PlanStep = {
  id: string;
  label: string;
  detail?: string;
  state: ExecutionState;
  statusLabel: string;
};

const planIcons = {
  idle: "clock",
  running: "loading",
  waiting: "clock",
  completed: "circle-check",
  failed: "circle-attention",
  cancelled: "close",
} as const;

export function AgentPlan({ title, steps }: { title: string; steps: readonly PlanStep[] }) {
  return (
    <AssistantPanel>
      <PanelHeading>{title}</PanelHeading>
      <AssistantProgress
        label={title}
        value={steps.filter((step) => step.state === "completed").length}
        max={Math.max(1, steps.length)}
      />
      <ol className="mt-4 space-y-3">
        {steps.map((step) => (
          <li key={step.id} className="flex items-start gap-3">
            <Icon
              name={planIcons[step.state]}
              className={
                step.state === "running" ? "motion-safe:animate-assistant-spin" : undefined
              }
            />
            <div className="min-w-0 flex-1">
              <p className="text-ui font-medium">{step.label}</p>
              {step.detail === undefined ? null : (
                <p className="text-xs text-text-muted">{step.detail}</p>
              )}
            </div>
            <ExecutionStatus state={step.state}>{step.statusLabel}</ExecutionStatus>
          </li>
        ))}
      </ol>
    </AssistantPanel>
  );
}

export type SubagentEntry = {
  id: string;
  name: string;
  task: string;
  model: string;
  progress: number;
  state: ExecutionState;
  statusLabel: string;
};

export function SubagentList({
  agents,
  label,
  onSelect,
  summaryAgent,
}: {
  agents: readonly SubagentEntry[];
  summaryAgent?: { name: string; model: string; label: string };
  label: string;
  onSelect: (id: string) => void;
}) {
  return (
    <AssistantPanel>
      <PanelHeading>{label}</PanelHeading>
      {summaryAgent === undefined ? null : (
        <div aria-busy="true" className="mb-3 space-y-2 motion-safe:animate-assistant-tool-step">
          <p>
            {summaryAgent.name} · {summaryAgent.model}
          </p>
          <progress aria-label={summaryAgent.label} className="w-full" />
        </div>
      )}
      <ul className="space-y-3">
        {agents.map((agent) => (
          <li key={agent.id} className="space-y-2">
            <div className="flex flex-wrap items-center gap-3">
              <Avatar kind="agent" label={agent.name} initials={agent.name.slice(0, 2)} />
              <Button
                variant="link"
                onClick={() => {
                  onSelect(agent.id);
                }}
              >
                {agent.name}
              </Button>
              <ExecutionStatus state={agent.state}>{agent.statusLabel}</ExecutionStatus>
            </div>
            <p className="text-ui">{agent.task}</p>
            <p className="text-xs text-text-muted">{agent.model}</p>
            <AssistantProgress label={agent.task} value={agent.progress} />
          </li>
        ))}
      </ul>
    </AssistantPanel>
  );
}

export function AgentStatus({
  state,
  label,
  elapsed,
  tasks,
  tasksLabel,
  summary,
  onLoadMore,
  loadMoreLabel,
}: {
  state: ExecutionState;
  label: string;
  elapsed?: string;
  tasks?: ReactNode;
  tasksLabel?: string;
  summary?: string;
  onLoadMore?: () => void;
  loadMoreLabel?: string;
}) {
  return (
    <span className="inline-flex flex-wrap items-center gap-2">
      <span key={label} className="motion-safe:animate-assistant-receipt">
        <ExecutionStatus state={state}>{label}</ExecutionStatus>
      </span>
      {summary}
      {tasks === undefined ? null : (
        <AssistantDisclosure summary={tasksLabel}>
          {tasks}
          {onLoadMore === undefined ? null : (
            <Button variant="ghost" onClick={onLoadMore}>
              {loadMoreLabel}
            </Button>
          )}
        </AssistantDisclosure>
      )}
      {elapsed === undefined ? null : (
        <span className="text-xs text-text-muted tabular-nums">{elapsed}</span>
      )}
    </span>
  );
}

export function TaskCard({
  title,
  state,
  statusLabel,
  duration,
  result,
  transcript,
  transcriptLabel,
  actions,
  footer,
  open,
  onOpenChange,
}: {
  title: string;
  state: ExecutionState;
  statusLabel: string;
  duration: string;
  result?: ReactNode;
  transcript?: ReactNode;
  transcriptLabel: string;
  actions?: ReactNode;
  footer?: ReactNode;
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
}) {
  return (
    <AssistantPanel>
      <PanelHeading actions={<AgentStatus state={state} label={statusLabel} elapsed={duration} />}>
        {title}
      </PanelHeading>
      {result}
      {actions}
      {transcript === undefined || transcript === null ? null : (
        <AssistantDisclosure
          className="mt-3"
          open={open}
          {...(onOpenChange === undefined ? {} : { onOpenChange })}
          summary={transcriptLabel}
        >
          {transcript}
        </AssistantDisclosure>
      )}
      {footer}
    </AssistantPanel>
  );
}

export type ApprovalState = "request" | "running" | "completed" | "denied";

const approvalIcons = {
  request: "screen",
  running: "loading",
  completed: "check",
  denied: "close",
} as const;

/** Explicit authorization lifecycle; applications execute once and own the final receipt. */
export function ApprovalCard({
  state,
  title,
  subtitle,
  description,
  command,
  details,
  allowOnceLabel,
  alwaysAllowLabel,
  denyLabel,
  onAllowOnce,
  onAlwaysAllow,
  onDeny,
  statusLabel,
  variant = "default",
  icon,
  className,
  ...props
}: Omit<ComponentPropsWithRef<"fieldset">, "children"> & {
  state: ApprovalState;
  title: string;
  subtitle: string;
  description?: string;
  command?: string;
  details?: readonly { label: string; value: string }[];
  allowOnceLabel: string;
  alwaysAllowLabel: string;
  denyLabel: string;
  onAllowOnce?: (() => void) | undefined;
  onAlwaysAllow?: () => void;
  onDeny?: () => void;
  statusLabel: string;
  variant?: "default" | "destructive";
  icon?: ReactNode;
}) {
  return (
    <fieldset
      {...props}
      className={cn(
        "min-w-0 space-y-3 rounded-panel border-0 bg-surface p-4 text-ui leading-ui text-ink",
        className,
      )}
    >
      <legend className="sr-only">{title}</legend>
      <div className="flex items-center gap-3">
        <span
          aria-hidden="true"
          className={cn(
            "flex size-control shrink-0 items-center justify-center rounded-control",
            variant === "destructive"
              ? "bg-danger-soft text-danger"
              : "bg-surface-muted text-text-muted",
          )}
        >
          {icon ?? (
            <Icon
              name={variant === "destructive" ? "triangle-attention" : "screen"}
              className="size-4"
            />
          )}
        </span>
        <div className="min-w-0">
          <p className="font-medium">{title}</p>
          <p className="text-xs text-text-muted">{subtitle}</p>
        </div>
      </div>
      {description === undefined ? null : <p className="text-text-muted">{description}</p>}
      {command === undefined ? null : (
        <pre className="overflow-x-auto rounded-control bg-surface-muted px-3 py-2.5 font-mono text-code leading-code">
          <code>{command}</code>
        </pre>
      )}
      {details === undefined || details.length === 0 ? null : (
        <dl className="space-y-2 rounded-control bg-surface-muted px-3 py-2.5 text-xs">
          {details.map((detail) => (
            <div key={detail.label} className="grid grid-cols-[minmax(0,1fr)_minmax(0,2fr)] gap-4">
              <dt className="font-mono text-text-muted">{detail.label}</dt>
              <dd className="break-words">{detail.value}</dd>
            </div>
          ))}
        </dl>
      )}
      <div className="flex min-h-control-small flex-wrap items-center justify-end gap-2">
        {state === "request" ? (
          <>
            {onDeny === undefined ? null : (
              <Button variant="ghost" size="sm" onClick={onDeny}>
                {denyLabel}
              </Button>
            )}
            {onAlwaysAllow === undefined ? null : (
              <Button variant="ghost" size="sm" onClick={onAlwaysAllow}>
                {alwaysAllowLabel}
              </Button>
            )}
            {onAllowOnce === undefined ? null : (
              <Button
                variant={variant === "destructive" ? "destructive" : "primary"}
                size="sm"
                onClick={onAllowOnce}
              >
                {allowOnceLabel}
              </Button>
            )}
          </>
        ) : (
          <output
            key={state}
            aria-live="polite"
            className="flex items-center gap-2 text-xs text-text-muted motion-safe:animate-assistant-receipt"
          >
            <Icon
              name={approvalIcons[state]}
              className={cn(
                "size-3.5",
                state === "running" && "motion-safe:animate-assistant-spin",
                state === "completed" && "text-success",
              )}
            />
            {statusLabel}
          </output>
        )}
      </div>
    </fieldset>
  );
}

export type OptionItem = { id: string; label: string; description?: string; disabled?: boolean };

export function OptionList({
  label,
  options,
  selected,
  mode = "single",
  onChange,
  disabled = false,
  pending = false,
  error,
  choice,
  confirmLabel,
  onConfirm,
  minSelections = 1,
  maxSelections = options.length,
}: {
  label: string;
  options: readonly OptionItem[];
  selected: readonly string[];
  mode?: "single" | "multiple";
  onChange: (ids: readonly string[]) => void;
  disabled?: boolean;
  pending?: boolean;
  error?: string;
  choice?: readonly string[];
  confirmLabel?: string;
  onConfirm?: (ids: readonly string[]) => void;
  minSelections?: number;
  maxSelections?: number;
}) {
  const chosen = choice ?? selected;
  const visible =
    choice === undefined ? options : options.filter((option) => choice.includes(option.id));
  return (
    <fieldset
      className="space-y-3"
      disabled={disabled || pending || choice !== undefined}
      aria-busy={pending}
    >
      <legend className="mb-3 font-medium">{label}</legend>
      {visible.map((option) => (
        <label
          key={option.id}
          className="flex min-h-control cursor-pointer items-start gap-3 rounded-pill border border-transparent bg-surface px-4 py-2 has-checked:bg-primary-soft has-disabled:cursor-not-allowed pointer-coarse:min-h-control-touch"
        >
          <SelectionControl
            type={mode === "single" ? "radio" : "checkbox"}
            name={label}
            value={option.id}
            disabled={option.disabled}
            checked={chosen.includes(option.id)}
            onChange={() => {
              if (mode === "single") {
                onChange([option.id]);
                onConfirm?.([option.id]);
                return;
              }
              if (!selected.includes(option.id) && selected.length >= maxSelections) {
                return;
              }
              onChange(
                selected.includes(option.id)
                  ? selected.filter((id) => id !== option.id)
                  : [...selected, option.id],
              );
            }}
            className="mt-0.5"
          />
          <span className="min-w-0">
            <span className="block text-ui font-medium">{option.label}</span>
            {option.description === undefined || option.description.length === 0 ? null : (
              <span className="block text-xs text-text-muted">{option.description}</span>
            )}
          </span>
        </label>
      ))}
      {error === undefined ? null : (
        <p role="alert" className="text-danger">
          {error}
        </p>
      )}
      {choice === undefined ? null : (
        <output className="motion-safe:animate-assistant-receipt">
          {visible.map((option) => option.label).join(", ")}
        </output>
      )}
      {onConfirm === undefined || mode === "single" || choice !== undefined ? null : (
        <Button
          loading={pending}
          loadingLabel={confirmLabel ?? label}
          disabled={selected.length < minSelections || selected.length > maxSelections}
          onClick={() => {
            onConfirm(selected);
          }}
        >
          {confirmLabel}
        </Button>
      )}
    </fieldset>
  );
}

export type Question = {
  id: string;
  label: string;
  options: readonly OptionItem[];
  mode?: "single" | "multiple";
  minSelections?: number;
  maxSelections?: number;
};

/** Current step and answers are controlled; the host decides when a receipt replaces the flow. */
export function QuestionFlow({
  questions,
  index,
  answers,
  onAnswer,
  onStepChange,
  onSubmit,
  backLabel,
  nextLabel,
  submitLabel,
  pending = false,
  error,
  receipt,
}: {
  questions: readonly Question[];
  index: number;
  answers: Readonly<Record<string, readonly string[]>>;
  onAnswer: (id: string, value: readonly string[]) => void;
  onStepChange: (index: number) => void;
  onSubmit: () => void;
  backLabel: string;
  nextLabel: string;
  submitLabel: string;
  pending?: boolean;
  error?: string;
  receipt?: ReactNode;
}) {
  const root = useRef<HTMLElement>(null);
  const previous = useRef(index);
  useLayoutEffect(() => {
    if (previous.current !== index) {
      previous.current = index;
      root.current?.querySelector<HTMLInputElement>("input:not(:disabled)")?.focus();
    }
  }, [index]);
  const question = questions[index];
  if (!question) {
    return null;
  }
  const last = index === questions.length - 1;
  if (receipt !== undefined) {
    return <output className="motion-safe:animate-assistant-receipt">{receipt}</output>;
  }
  return (
    <AssistantPanel ref={root} aria-busy={pending}>
      <AssistantProgress label={question.label} value={index + 1} max={questions.length} />
      <p className="mb-3 text-xs text-text-muted tabular-nums">
        {index + 1} / {questions.length}
      </p>
      <OptionList
        label={question.label}
        mode={question.mode ?? "single"}
        maxSelections={question.maxSelections ?? question.options.length}
        disabled={pending}
        options={question.options}
        selected={answers[question.id] ?? []}
        onChange={(value) => {
          onAnswer(question.id, value);
        }}
      />
      <div className="mt-4 flex flex-wrap justify-between gap-2">
        <Button
          variant="ghost"
          disabled={pending || index === 0}
          onClick={() => {
            onStepChange(index - 1);
          }}
        >
          {backLabel}
        </Button>
        <Button
          loading={pending}
          loadingLabel={submitLabel}
          disabled={(answers[question.id]?.length ?? 0) < (question.minSelections ?? 1)}
          onClick={() => {
            if (last) {
              onSubmit();
            } else {
              onStepChange(index + 1);
            }
          }}
        >
          {last ? submitLabel : nextLabel}
        </Button>
      </div>
      {error === undefined ? null : (
        <p role="alert" className="text-danger">
          {error}
        </p>
      )}
    </AssistantPanel>
  );
}

export function RecommendationCard({
  confidence,
  onAccept,
  onAlternatives,
  alternativesLabel,
  ...props
}: React.ComponentProps<typeof ApprovalCard> & {
  confidence: string;
  onAccept?: () => void;
  onAlternatives?: () => void;
  alternativesLabel?: string;
}) {
  return (
    <div className="space-y-2">
      <Badge tone="agent">{confidence}</Badge>
      <ApprovalCard
        {...props}
        {...((onAccept ?? props.onAllowOnce) === undefined
          ? {}
          : { onAllowOnce: onAccept ?? props.onAllowOnce })}
      />
      {props.state !== "request" || onAlternatives === undefined ? null : (
        <Button variant="ghost" onClick={onAlternatives}>
          {alternativesLabel}
        </Button>
      )}
    </div>
  );
}

export function ArtifactCard({
  title,
  detail,
  version,
  state,
  statusLabel,
  openLabel,
  onOpen,
  preview,
}: {
  title: string;
  detail: string;
  version: string;
  state: ExecutionState;
  statusLabel: string;
  openLabel: string;
  onOpen: () => void;
  preview?: ReactNode;
}) {
  return (
    <AssistantPanel className="relative transition-transform duration-150 hover:-translate-y-px active:scale-[0.98] motion-reduce:transition-none">
      <PanelHeading actions={<ExecutionStatus state={state}>{statusLabel}</ExecutionStatus>}>
        <span className="flex items-center gap-2">
          <Icon
            name="file"
            className={state === "running" ? "motion-safe:animate-pulse" : undefined}
          />
          {title}
        </span>
      </PanelHeading>
      <p key={detail} className="text-ui text-text-muted motion-safe:animate-assistant-receipt">
        {detail}
      </p>
      <p className="mt-1 text-xs text-text-muted">{version}</p>
      {preview}
      <Button
        aria-label={openLabel}
        className="absolute inset-0 size-full rounded-panel"
        variant="ghost"
        onClick={onOpen}
      >
        <span className="sr-only">{openLabel}</span>
      </Button>
    </AssistantPanel>
  );
}

export type TodoItem = {
  id: string;
  label: string;
  state: "pending" | "active" | "done" | "failed" | "cancelled";
  description?: string;
  reason?: string;
};
const todoIcons = {
  pending: "clock",
  active: "loading",
  done: "check",
  failed: "circle-attention",
  cancelled: "close",
} as const;

export function TodoList({
  label,
  items,
  maxVisible = items.length,
  expandLabel = "Show all tasks",
  collapseLabel = "Show less",
}: {
  label: string;
  items: readonly TodoItem[];
  maxVisible?: number;
  expandLabel?: string;
  collapseLabel?: string;
}) {
  const [expanded, setExpanded] = useState(false);
  const visible = expanded
    ? items
    : items.filter(
        (item, index) => index < maxVisible || item.state === "active" || item.state === "failed",
      );
  return (
    <AssistantPanel>
      <PanelHeading
        actions={
          <span className="font-mono text-xs">
            {items.filter((item) => item.state === "done").length}/
            {items.filter((item) => item.state !== "cancelled").length}
          </span>
        }
      >
        {label}
      </PanelHeading>
      <ul className="space-y-2">
        {visible.map((item) => (
          <li
            key={item.id}
            className="flex items-start gap-2 motion-safe:animate-assistant-tool-step"
          >
            <Icon
              name={todoIcons[item.state]}
              className={item.state === "active" ? "motion-safe:animate-assistant-spin" : undefined}
            />
            <span>
              <span
                className={cn(
                  "text-ui",
                  item.state === "cancelled" && "text-text-muted line-through",
                )}
              >
                {item.label}
              </span>
              <span className="block text-xs text-text-muted">{item.description}</span>
              <span className="block text-xs text-danger">{item.reason}</span>
              <span className="sr-only">{item.state}</span>
            </span>
          </li>
        ))}
      </ul>
      {items.length > maxVisible ? (
        <Button
          variant="ghost"
          aria-expanded={expanded}
          onClick={() => {
            setExpanded(!expanded);
          }}
        >
          {expanded ? collapseLabel : expandLabel}
        </Button>
      ) : null}
    </AssistantPanel>
  );
}

export function AgentCard({
  name,
  initials,
  description,
  skills,
  model,
  endpoint,
  actionLabel,
  onAction,
  connected = false,
  provider,
  version,
  skillDescriptions,
}: {
  name: string;
  initials: string;
  description: string;
  skills: readonly string[];
  skillDescriptions?: Readonly<Record<string, string>>;
  provider?: string;
  version?: string;
  connected?: boolean;
  model: string;
  endpoint: string;
  actionLabel: string;
  onAction: () => void;
}) {
  return (
    <AssistantPanel>
      <PanelHeading>
        <span className="flex items-center gap-3">
          <Avatar kind="agent" label={name} initials={initials} />
          {name}
          <Badge tone="agent">Agent</Badge>
        </span>
      </PanelHeading>
      <p className="text-xs text-text-muted">
        {provider} {version}
      </p>
      <p className="mb-4 text-ui text-text-muted">{description}</p>
      <ul className="mb-4 flex flex-wrap gap-2">
        {skills.map((skill) => (
          <li key={skill}>
            <Badge>{skill}</Badge>
            <p className="text-xs text-text-muted">{skillDescriptions?.[skill]}</p>
          </li>
        ))}
      </ul>
      <Facts
        items={[
          { label: "Model", value: model },
          { label: "Endpoint", value: endpoint },
        ]}
      />
      <Button className="mt-4" disabled={connected} onClick={onAction}>
        {connected ? <Icon name="check" /> : null}
        {actionLabel}
      </Button>
    </AssistantPanel>
  );
}

export function AgentHandoff({
  from,
  to,
  reason,
  context,
  settled = false,
  label,
}: {
  settled?: boolean;
  from: string;
  to: string;
  reason: string;
  context: readonly string[];
  label: string;
}) {
  return (
    <AssistantDisclosure
      summary={
        <span className="flex flex-wrap items-center gap-2">
          <span
            className={cn(
              "transition-opacity duration-500 motion-reduce:transition-none",
              settled && "opacity-45",
            )}
          >
            {from}
          </span>
          <Icon
            name="arrow"
            className={cn(
              "transition-colors duration-500 motion-reduce:transition-none",
              !settled && "text-primary",
            )}
          />
          <span
            className={cn(
              "rounded-pill px-2 transition-colors duration-500 motion-reduce:transition-none",
              !settled && "bg-primary-soft text-primary",
            )}
          >
            {to}
          </span>
        </span>
      }
    >
      <p className="mb-3 text-ui">{reason}</p>
      <h4 className="mb-2 text-xs font-medium text-text-muted">{label}</h4>
      <ul className="list-disc space-y-1 pl-5 text-ui">
        {context.map((entry) => (
          <li key={entry}>{entry}</li>
        ))}
      </ul>
    </AssistantDisclosure>
  );
}

export type BackgroundRun = {
  id: string;
  title: string;
  state: ExecutionState;
  statusLabel: string;
  detail: string;
};

const backgroundSummary = (ready: number, running: number) => `${ready} ready · ${running} running`;

export function BackgroundRuns({
  label,
  runs,
  collectLabel,
  onCollect,
  summaryLabel = backgroundSummary,
}: {
  label: string;
  runs: readonly BackgroundRun[];
  collectLabel: string;
  onCollect: (id: string) => void;
  summaryLabel?: (ready: number, running: number) => string;
}) {
  return (
    <AssistantPanel>
      <PanelHeading
        actions={
          <span className="text-xs text-text-muted">
            {summaryLabel(
              runs.filter((run) => run.state === "completed").length,
              runs.filter((run) => run.state === "running").length,
            )}
          </span>
        }
      >
        {label}
      </PanelHeading>
      <ul className="space-y-3">
        {runs.map((run) => (
          <li key={run.id}>
            <Button
              variant="ghost"
              className="h-auto w-full justify-start gap-3 rounded-panel whitespace-normal"
              disabled={run.state === "running" || run.state === "waiting" || run.state === "idle"}
              onClick={() => {
                onCollect(run.id);
              }}
            >
              <Icon
                name={planIcons[run.state]}
                className={
                  run.state === "running" ? "motion-safe:animate-assistant-spin" : undefined
                }
              />
              <span className="min-w-0 flex-1 text-left">
                <span className="block font-medium">{run.title}</span>
                <span className="block text-xs text-text-muted">{run.detail}</span>
              </span>
              <ExecutionStatus state={run.state}>{run.statusLabel}</ExecutionStatus>
              <span className="sr-only">{collectLabel}</span>
            </Button>
          </li>
        ))}
      </ul>
    </AssistantPanel>
  );
}

export type Checkpoint = { id: string; title: string; detail: string; time: string };

export function CheckpointHistory({
  label,
  checkpoints,
  restoreLabel,
  onRestore,
  currentId,
  currentLabel = "Current",
}: {
  label: string;
  checkpoints: readonly Checkpoint[];
  currentId?: string;
  currentLabel?: string;
  restoreLabel: string;
  onRestore: (id: string) => void;
}) {
  return (
    <AssistantPanel>
      <PanelHeading>{label}</PanelHeading>
      <ol className="space-y-3">
        {checkpoints.map((checkpoint) => (
          <li key={checkpoint.id} className="flex flex-wrap items-center gap-3">
            <Icon name={currentId === checkpoint.id ? "check" : "history"} />
            <div className="min-w-0 flex-1">
              <p className="text-ui font-medium">{checkpoint.title}</p>
              <p className="text-xs text-text-muted">
                {checkpoint.detail} · {checkpoint.time}
              </p>
            </div>
            <Button
              variant="secondary"
              disabled={currentId === checkpoint.id}
              onClick={() => {
                onRestore(checkpoint.id);
              }}
            >
              {currentId === checkpoint.id ? currentLabel : restoreLabel}
            </Button>
          </li>
        ))}
      </ol>
    </AssistantPanel>
  );
}

export function ScheduleCard({
  title,
  cadence,
  nextRun,
  enabled,
  enabledLabel,
  onEnabledChange,
  history,
  runs,
}: {
  title: string;
  cadence: string;
  nextRun: string;
  enabled: boolean;
  enabledLabel: string;
  onEnabledChange: (enabled: boolean) => void;
  history?: ReactNode;
  runs?: readonly {
    id: string;
    time: string;
    state: "completed" | "failed";
    statusLabel: string;
  }[];
}) {
  return (
    <AssistantPanel>
      <PanelHeading>{title}</PanelHeading>
      <Facts
        items={[
          { label: "Cadence", value: cadence },
          { label: "Next run", value: nextRun },
        ]}
      />
      <label className="my-3 flex min-h-control cursor-pointer items-center gap-3 text-ui pointer-coarse:min-h-control-touch">
        <SelectionControl
          type="checkbox"
          checked={enabled}
          onChange={(event) => {
            onEnabledChange(event.target.checked);
          }}
        />
        {enabledLabel}
      </label>
      {history}
      <ul className="space-y-2">
        {runs?.map((run) => (
          <li key={run.id} className="flex gap-2">
            <span>{run.time}</span>
            <ExecutionStatus state={run.state}>{run.statusLabel}</ExecutionStatus>
          </li>
        ))}
      </ul>
    </AssistantPanel>
  );
}
