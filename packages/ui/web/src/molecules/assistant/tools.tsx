import { useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import type { ReactNode } from "react";

import { Badge } from "../../atoms/badge";
import { Button } from "../../atoms/button";
import { Icon } from "../../atoms/icon";
import type { IconName } from "../../atoms/icon";
import { Input } from "../../atoms/input";
import {
  SelectContent,
  SelectItem,
  SelectRoot,
  SelectTrigger,
  SelectValue,
} from "../../atoms/select";
import { SelectionControl } from "../../atoms/selection-control";
import { Textarea } from "../../atoms/textarea";
import { cn } from "../../classes";
import { Field } from "../field";
import { FileDiff } from "./file-diff";
import type { FileDiffRow } from "./file-diff";
import { MessageError } from "./messages";
import {
  AssistantDisclosure,
  AssistantPanel,
  CodeSurface,
  ExecutionStatus,
  Facts,
  PanelHeading,
} from "./shared";
import type { ExecutionState } from "./shared";
import { parseAnsi } from "./terminal-ansi";
import { ThinkingState } from "./thinking-state";

/** Two overlapping labels preserve the reference's 300ms blur/fade and measured-width swap. */
function ToolActivityLabel({
  running,
  activeLabel,
  label,
}: {
  running: boolean;
  activeLabel: string;
  label: string;
}) {
  const container = useRef<HTMLSpanElement>(null);
  const active = useRef<HTMLSpanElement>(null);
  const resting = useRef<HTMLSpanElement>(null);
  useLayoutEffect(() => {
    const outer = container.current;
    const layer = running ? active.current : resting.current;
    if (outer === null || layer === null) {
      return undefined;
    }
    const preference = window.matchMedia("(prefers-reduced-motion: reduce)");
    let animation: Animation | undefined;
    const measure = () => {
      const start = outer.getBoundingClientRect().width;
      const end = Math.ceil(layer.getBoundingClientRect().width);
      animation?.cancel();
      animation = outer.animate([{ width: `${start}px` }, { width: `${end}px` }], {
        duration: preference.matches ? 0 : 300,
        easing: "cubic-bezier(0.23,1,0.32,1)",
        fill: "forwards",
      });
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(layer);
    preference.addEventListener("change", measure);
    return () => {
      observer.disconnect();
      preference.removeEventListener("change", measure);
      animation?.cancel();
    };
  }, [running]);
  const layerClass =
    "col-start-1 row-start-1 w-max transition-all duration-300 ease-assistant-label motion-reduce:transition-none";
  return (
    <span ref={container} className="grid min-w-0 overflow-x-clip text-left">
      <span
        ref={active}
        aria-hidden={!running}
        className={cn(
          layerClass,
          running
            ? "opacity-100 blur-none"
            : "pointer-events-none opacity-0 blur-tooltip select-none",
        )}
      >
        <ThinkingState>{activeLabel}</ThinkingState>
      </span>
      <span
        ref={resting}
        aria-hidden={running}
        className={cn(
          layerClass,
          running
            ? "pointer-events-none opacity-0 blur-tooltip select-none"
            : "opacity-100 blur-none",
        )}
      >
        {label}
      </span>
    </span>
  );
}

export type ToolCallProps = {
  name: string;
  activeLabel: string;
  query?: string;
  interrupt?: ReactNode;
  error?: ReactNode;
  state: ExecutionState;
  statusLabel: string;
  request: ReactNode;
  result?: ReactNode;
  requestLabel: string;
  resultLabel: string;
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
};

/** The host owns execution; this compact disclosure renders its request and result. */
export function ToolCall({
  name,
  activeLabel,
  query,
  interrupt,
  error,
  state,
  statusLabel,
  request,
  result,
  requestLabel,
  resultLabel,
  open,
  onOpenChange,
}: ToolCallProps) {
  const running = state === "running";
  return (
    <details
      open={open}
      onToggle={(event) => {
        if (open === undefined) {
          onOpenChange?.(event.currentTarget.open);
        }
      }}
      className="assistant-disclosure group min-w-0 text-ui text-text-muted"
      data-motion="tool"
      aria-busy={running}
    >
      <summary
        onClick={(event) => {
          if (open !== undefined) {
            event.preventDefault();
            onOpenChange?.(!open);
          }
        }}
        className="flex min-h-control cursor-pointer list-none items-center gap-2 rounded-pill px-2 py-1 text-ui leading-ui transition-colors duration-150 hover:bg-surface-hover hover:text-ink focus-visible:outline-2 focus-visible:outline-primary motion-reduce:transition-none pointer-coarse:min-h-control-touch"
      >
        <Icon
          name="chevron"
          className="size-3.5 -rotate-90 transition-transform duration-200 ease-swarmie group-open:rotate-0 motion-reduce:transition-none"
        />
        <ToolActivityLabel running={running} activeLabel={activeLabel} label={name} />
        {query === undefined ? null : (
          <span className="min-w-0 truncate rounded-pill bg-surface-muted px-2 py-0.5 font-mono text-code text-ink">
            {query}
          </span>
        )}
        <span className="ml-auto flex shrink-0 items-center gap-1">
          <span className="sr-only">{statusLabel}</span>
          {state === "completed" ? (
            <Icon
              name="check"
              className="size-3.5 text-success motion-safe:animate-assistant-pop160"
            />
          ) : null}
          {state === "cancelled" ? <Icon name="close" className="size-3.5" /> : null}
          {state === "waiting" ? <Icon name="clock" className="size-3.5 text-attention" /> : null}
          {running ? (
            <Icon name="loading" className="size-3.5 motion-safe:animate-assistant-spin" />
          ) : null}
          {state === "failed" ? (
            <Icon name="circle-attention" className="size-3.5 text-danger" />
          ) : null}
        </span>
      </summary>
      {interrupt}
      {error}
      <div className="mt-2 overflow-hidden rounded-panel bg-surface-muted text-ui">
        <div className="px-3 py-2.5">
          <h4 className="mb-1 font-mono text-xs text-text-muted">{requestLabel}</h4>
          <div className="min-w-0 break-words">{request}</div>
        </div>
        {result === undefined || result === null ? null : (
          <div className="px-3 py-2.5">
            <h4 className="mb-1 font-mono text-xs text-text-muted">{resultLabel}</h4>
            <div className="min-w-0 break-words text-ink">{result}</div>
          </div>
        )}
      </div>
    </details>
  );
}

export type ToolTimelineEntry = {
  id: string;
  action: string;
  icon?: IconName;
  target: string;
  detail?: string;
  state: ExecutionState;
  statusLabel: string;
};

export function ToolTimeline({
  entries,
  label,
  activeLabel,
  visibleEntries = entries.length,
  open,
  onOpenChange,
  stats,
}: {
  entries: readonly ToolTimelineEntry[];
  label: string;
  activeLabel: string;
  visibleEntries?: number;
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
  stats?: readonly { file: string; added?: number; removed?: number }[];
}) {
  const shown = entries.slice(0, Math.max(0, visibleEntries));
  const running = shown.some((entry) => entry.state === "running");
  return (
    <details
      open={open}
      onToggle={(event) => {
        if (open === undefined) {
          onOpenChange?.(event.currentTarget.open);
        }
      }}
      className="assistant-disclosure group min-w-0 text-ui text-text-muted"
      data-motion="tool"
    >
      <summary
        onClick={(event) => {
          if (open !== undefined) {
            event.preventDefault();
            onOpenChange?.(!open);
          }
        }}
        className="flex min-h-control cursor-pointer list-none items-center gap-2 rounded-pill px-2 py-1 text-ui leading-ui transition-colors duration-150 hover:bg-surface-hover hover:text-ink focus-visible:outline-2 focus-visible:outline-primary motion-reduce:transition-none pointer-coarse:min-h-control-touch"
      >
        <Icon
          name="chevron"
          className="size-3.5 -rotate-90 transition-transform duration-200 ease-swarmie group-open:rotate-0 motion-reduce:transition-none"
        />
        <ToolActivityLabel running={running} activeLabel={activeLabel} label={label} />
      </summary>
      <ol aria-label={label} className="space-y-2.5 pt-2.5 pl-4">
        {shown.map((entry) => (
          <li
            key={entry.id}
            className="flex min-w-0 items-center gap-2 motion-safe:animate-assistant-tool-step"
          >
            <Icon
              name={entry.icon ?? (entry.state === "failed" ? "circle-attention" : "task")}
              className="size-3.5"
            />
            <span className="sr-only">{entry.statusLabel}</span>
            {entry.state === "running" ? (
              <ThinkingState>{entry.action}</ThinkingState>
            ) : (
              <span>{entry.action}</span>
            )}
            <span
              className="min-w-0 truncate rounded-pill bg-surface-muted px-2 py-0.5 font-mono text-code text-ink"
              title={entry.detail}
            >
              {entry.target}
            </span>
          </li>
        ))}
      </ol>
      {stats === undefined || stats.length === 0 ? null : (
        <div className="flex flex-wrap gap-1.5 pt-3 pl-4">
          {stats.map((stat) => (
            <span
              key={stat.file}
              className="inline-flex min-w-0 items-center gap-1 rounded-pill bg-surface-muted px-2 py-0.5 font-mono text-code"
            >
              <span className="truncate">{stat.file}</span>
              {stat.added === undefined ? null : (
                <span className="text-success">+{stat.added}</span>
              )}
              {stat.removed === undefined ? null : (
                <span className="text-danger">−{stat.removed}</span>
              )}
            </span>
          ))}
        </div>
      )}
    </details>
  );
}

export type TerminalLine = { id: string; text: string; stream?: "stdout" | "stderr" };

const ansiTones = new Map<number, string>(
  Object.entries({
    31: "text-danger",
    91: "text-danger",
    32: "text-success",
    92: "text-success",
    33: "text-attention",
    93: "text-attention",
    34: "text-primary",
    94: "text-primary",
    35: "text-primary",
    95: "text-primary",
    36: "text-primary",
    96: "text-primary",
    90: "text-text-muted",
  }).map(([code, tone]) => [Number(code), tone]),
);

function terminalTone(exitCode: number | undefined): "agent" | "success" | "danger" {
  if (exitCode === undefined) {
    return "agent";
  }
  return exitCode === 0 ? "success" : "danger";
}

export function TerminalBlock({
  command,
  lines,
  statusLabel,
  exitCode,
  running = false,
  cwd,
  duration,
  visibleCount,
  maxCollapsedLines = 12,
  copyLabel = "Copy output",
  expandLabel = "Show all lines",
  collapseLabel = "Show less",
  onCopy,
  truncatedLabel,
}: {
  command: string;
  lines: readonly TerminalLine[];
  statusLabel: string;
  exitCode?: number | undefined;
  running?: boolean;
  cwd?: string;
  duration?: string | undefined;
  visibleCount?: number;
  maxCollapsedLines?: number;
  copyLabel?: string;
  expandLabel?: string;
  collapseLabel?: string;
  onCopy?: (text: string) => void;
  truncatedLabel?: string;
}) {
  const [expanded, setExpanded] = useState(false);
  const outputId = useId();
  const entries = lines;
  const shown = entries.slice(
    0,
    visibleCount === undefined ? entries.length : Math.max(0, visibleCount),
  );
  const limit = Math.max(0, maxCollapsedLines);
  const canCollapse = !running && shown.length > limit;
  const displayed = canCollapse && !expanded ? shown.slice(0, limit) : shown;
  const tone = terminalTone(exitCode);
  return (
    <AssistantPanel aria-busy={running}>
      <PanelHeading
        actions={
          <span className="flex items-center gap-2">
            {running ? (
              <Icon name="loading" className="motion-safe:animate-assistant-spin" />
            ) : null}
            <Badge tone={tone}>{statusLabel}</Badge>
            {duration}
            {onCopy === undefined ? null : (
              <Button
                variant="ghost"
                onClick={() => {
                  onCopy(
                    entries
                      .map((line) =>
                        parseAnsi(line.text)
                          .map((part) => part.text)
                          .join(""),
                      )
                      .join("\n"),
                  );
                }}
              >
                {copyLabel}
              </Button>
            )}
          </span>
        }
      >
        <span className="font-mono text-code">
          {cwd === undefined ? null : <span className="text-text-muted">{cwd} $ </span>}
          {command}
        </span>
      </PanelHeading>
      <div
        id={outputId}
        className="overflow-x-auto rounded-control bg-surface-muted p-3 font-mono text-code leading-code"
      >
        {displayed.map((line) => (
          <div
            key={line.id}
            className={cn(
              "whitespace-pre-wrap motion-safe:animate-assistant-receipt",
              line.stream === "stderr" && "text-danger",
            )}
          >
            {parseAnsi(line.text).map((part) => (
              <span
                key={part.offset}
                className={cn(
                  line.stream !== "stderr" && part.color !== undefined && ansiTones.get(part.color),
                  part.bold && "font-bold",
                  part.dim && "opacity-60",
                )}
              >
                {part.text}
              </span>
            ))}
          </div>
        ))}
        {running ? (
          <span
            aria-hidden="true"
            className="inline-block h-3 w-1.5 bg-primary motion-safe:animate-pulse"
          />
        ) : null}
      </div>
      {canCollapse ? (
        <Button
          variant="ghost"
          aria-controls={outputId}
          aria-expanded={expanded}
          onClick={() => {
            setExpanded(!expanded);
          }}
        >
          {expanded ? collapseLabel : expandLabel}
        </Button>
      ) : null}
      {truncatedLabel === undefined ? null : (
        <p className="text-xs text-text-muted">{truncatedLabel}</p>
      )}
    </AssistantPanel>
  );
}

export type ReviewableHunk = {
  id: string;
  filename: string;
  rows: readonly FileDiffRow[];
  decision: "pending" | "keep" | "discard";
  range?: string;
};

export function ReviewableDiff({
  hunks,
  keepLabel,
  discardLabel,
  applyLabel,
  describeRow,
  onDecision,
  onApply,
  summaryLabel,
}: {
  hunks: readonly ReviewableHunk[];
  keepLabel: string;
  discardLabel: string;
  applyLabel: string;
  describeRow: (row: FileDiffRow) => string;
  onDecision: (id: string, decision: "keep" | "discard") => void;
  onApply: (ids: readonly string[]) => void;
  summaryLabel?: (kept: number, total: number) => string;
}) {
  const kept = hunks.filter((hunk) => hunk.decision === "keep").map((hunk) => hunk.id);
  return (
    <div className="space-y-3">
      <output className="text-xs text-text-muted">
        {summaryLabel?.(kept.length, hunks.length)}
      </output>
      {hunks.map((hunk) => (
        <div
          key={hunk.id}
          className={cn(
            "space-y-2 transition-opacity duration-300 motion-reduce:transition-none",
            hunk.decision === "discard" && "opacity-50",
          )}
        >
          {hunk.range === undefined ? null : <p className="font-mono text-xs">{hunk.range}</p>}
          {hunk.decision === "pending" ? null : (
            <output className="text-xs motion-safe:animate-assistant-receipt">
              {hunk.decision === "keep" ? keepLabel : discardLabel}
            </output>
          )}
          <FileDiff
            file={hunk.filename}
            rows={hunk.rows}
            label={hunk.filename}
            describeRow={describeRow}
          />
          <div className="flex flex-wrap gap-2">
            <Button
              variant="secondary"
              aria-pressed={hunk.decision === "keep"}
              onClick={() => {
                onDecision(hunk.id, "keep");
              }}
            >
              {keepLabel}
            </Button>
            <Button
              variant="ghost"
              aria-pressed={hunk.decision === "discard"}
              onClick={() => {
                onDecision(hunk.id, "discard");
              }}
            >
              {discardLabel}
            </Button>
          </div>
        </div>
      ))}
      <Button
        disabled={kept.length === 0 || hunks.some((hunk) => hunk.decision === "pending")}
        onClick={() => {
          onApply(kept);
        }}
      >
        {applyLabel}
      </Button>
    </div>
  );
}

export type FileTreeNode = {
  id: string;
  name: string;
  additions?: number;
  removals?: number;
  children?: readonly FileTreeNode[];
  onOpen?: () => void;
};

export function FileTree({ nodes, label }: { nodes: readonly FileTreeNode[]; label: string }) {
  return (
    <ul aria-label={label} className="space-y-1">
      {nodes.map((node) => (
        <li key={node.id} className="motion-safe:animate-assistant-tool-step">
          {node.children ? (
            <AssistantDisclosure
              summary={
                <span className="flex items-center gap-2">
                  <Icon name="project" />
                  {node.name}
                </span>
              }
            >
              <FileTree nodes={node.children} label={node.name} />
            </AssistantDisclosure>
          ) : (
            <div className="flex items-center gap-2 py-1">
              <Icon name="file" />
              {node.onOpen ? (
                <Button variant="link" onClick={node.onOpen}>
                  {node.name}
                </Button>
              ) : (
                <span className="min-w-0 flex-1 text-ui break-all">{node.name}</span>
              )}
              <span className="text-xs text-success tabular-nums">+{node.additions ?? 0}</span>
              <span className="text-xs text-danger tabular-nums">−{node.removals ?? 0}</span>
            </div>
          )}
        </li>
      ))}
    </ul>
  );
}

export type ElicitationField = {
  id: string;
  label: string;
  value: string;
  required?: boolean;
  error?: string;
  kind: "text" | "multiline" | "choice" | "date" | "radio" | "checkbox";
  readOnly?: boolean;
  disabled?: boolean;
  options?: readonly { value: string; label: string }[];
};

/** Controlled schema-derived fields. The application owns validation and submission. */
export function ElicitationForm({
  title,
  description,
  fields,
  onChange,
  submitLabel,
  cancelLabel,
  onSubmit,
  onCancel,
  disabled = false,
  state = "request",
  statusLabel,
  submitDisabled = false,
  cancelDisabled = false,
}: {
  title: string;
  description: string;
  fields: readonly ElicitationField[];
  onChange: (id: string, value: string) => void;
  submitLabel: string;
  cancelLabel: string;
  onSubmit: (values: Readonly<Record<string, string>>) => void;
  onCancel: () => void;
  disabled?: boolean;
  state?: "request" | "accepted" | "declined";
  submitDisabled?: boolean;
  cancelDisabled?: boolean;
  statusLabel?: string;
}) {
  return (
    <AssistantPanel>
      <form
        onSubmit={(event) => {
          event.preventDefault();
          if (!disabled && !submitDisabled && state === "request") {
            onSubmit(Object.fromEntries(fields.map((field) => [field.id, field.value])));
          }
        }}
        className="space-y-3"
      >
        <PanelHeading>{title}</PanelHeading>
        <p className="text-ui text-text-muted">{description}</p>
        {fields.map((field) => (
          <Field key={field.id} id={field.id} label={field.label} error={field.error}>
            {(control) => {
              const props = {
                ...control,
                name: field.id,
                value: field.value,
                required: field.required,
                readOnly: field.readOnly,
                disabled: disabled || (field.disabled ?? false) || state !== "request",
                onChange: (
                  event: React.ChangeEvent<
                    HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement
                  >,
                ) => {
                  onChange(field.id, event.target.value);
                },
              };
              if (field.kind === "checkbox") {
                return (
                  <SelectionControl
                    {...control}
                    name={field.id}
                    type="checkbox"
                    checked={field.value === "true"}
                    required={field.required}
                    disabled={props.disabled || field.readOnly}
                    onChange={(event) => {
                      onChange(field.id, String(event.target.checked));
                    }}
                  />
                );
              }
              if (field.kind === "radio") {
                return (
                  <fieldset {...control} aria-label={field.label} className="space-y-2">
                    <legend className="sr-only">{field.label}</legend>
                    {field.options?.map((option) => (
                      <label key={option.value} className="flex items-center gap-2">
                        <SelectionControl
                          name={field.id}
                          type="radio"
                          value={option.value}
                          checked={field.value === option.value}
                          required={field.required}
                          disabled={props.disabled || field.readOnly}
                          onChange={() => {
                            onChange(field.id, option.value);
                          }}
                        />
                        {option.label}
                      </label>
                    ))}
                  </fieldset>
                );
              }
              if (field.kind === "choice") {
                return (
                  <SelectRoot
                    name={field.id}
                    value={field.value}
                    required={field.required ?? false}
                    disabled={Boolean(props.disabled || field.readOnly)}
                    onValueChange={(value) => {
                      onChange(field.id, value);
                    }}
                  >
                    <SelectTrigger {...control} className="w-full">
                      <SelectValue>
                        {field.options?.find((option) => option.value === field.value)?.label}
                      </SelectValue>
                    </SelectTrigger>
                    <SelectContent>
                      {field.options?.map((option) => (
                        <SelectItem key={option.value} value={option.value}>
                          {option.label}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </SelectRoot>
                );
              }
              if (field.kind === "multiline") {
                return <Textarea {...props} />;
              }
              return <Input {...props} type={field.kind === "date" ? "date" : "text"} />;
            }}
          </Field>
        ))}
        {state === "request" ? (
          <div className="flex flex-wrap gap-2">
            <Button type="submit" disabled={disabled || submitDisabled}>
              {submitLabel}
            </Button>
            <Button variant="ghost" disabled={disabled || cancelDisabled} onClick={onCancel}>
              {cancelLabel}
            </Button>
          </div>
        ) : (
          <output aria-live="polite" className="motion-safe:animate-assistant-receipt">
            {statusLabel}
          </output>
        )}
      </form>
    </AssistantPanel>
  );
}

export type ServerConnection = {
  id: string;
  name: string;
  state: ExecutionState;
  statusLabel: string;
  tools: readonly string[];
  actionLabel?: string;
  onAction?: () => void;
  transport?: string;
  error?: string;
  removeLabel?: string;
  onRemove?: () => void;
};

export function ServerPanel({
  servers,
  label,
}: {
  servers: readonly ServerConnection[];
  label: string;
}) {
  return (
    <AssistantPanel aria-label={label}>
      <PanelHeading>{label}</PanelHeading>
      <ul className="space-y-3">
        {servers.map((server) => (
          <li key={server.id} className="space-y-2">
            <div className="flex flex-wrap items-center gap-3">
              <Icon
                name={server.state === "running" ? "loading" : "world"}
                className={
                  server.state === "running" ? "motion-safe:animate-assistant-spin" : undefined
                }
              />
              <span className="flex-1 text-ui font-medium">{server.name}</span>
              <ExecutionStatus state={server.state}>{server.statusLabel}</ExecutionStatus>
              {server.actionLabel !== undefined && server.onAction ? (
                <Button variant="secondary" onClick={server.onAction}>
                  {server.actionLabel}
                </Button>
              ) : null}
            </div>
            {server.error === undefined ? null : (
              <p role="alert" className="text-danger">
                {server.error}
              </p>
            )}
            {server.transport}
            {server.onRemove === undefined ? null : (
              <Button variant="ghost" onClick={server.onRemove}>
                {server.removeLabel}
              </Button>
            )}
            <AssistantDisclosure summary={`${server.tools.length} tools`}>
              <ul className="space-y-2">
                {server.tools.map((tool) => (
                  <li
                    key={tool}
                    className="font-mono text-code motion-safe:animate-assistant-receipt"
                  >
                    {tool}
                  </li>
                ))}
              </ul>
            </AssistantDisclosure>
          </li>
        ))}
      </ul>
    </AssistantPanel>
  );
}

export function ToolFailure({
  attempts,
  attempt,
  maxAttempts,
  target,
  skipLabel,
  onSkip,
  ...props
}: React.ComponentProps<typeof MessageError> & {
  attempts: string;
  attempt?: number;
  maxAttempts?: number;
  target?: string;
  skipLabel?: string;
  onSkip?: () => void;
}) {
  return (
    <div className="space-y-2">
      <MessageError {...props} />
      <p className="text-xs text-text-muted">
        {target} {attempts}
        {attempt === undefined ? null : (
          <span className="ml-2 font-mono">
            {attempt}/{maxAttempts}
          </span>
        )}
      </p>
      {onSkip === undefined ? null : (
        <Button variant="ghost" disabled={props.retrying} onClick={onSkip}>
          {skipLabel}
        </Button>
      )}
    </div>
  );
}

export function PermissionGrant({
  title,
  scope,
  children,
  grantLabel,
  denyLabel,
  onGrant,
  onDeny,
  onAlways,
  alwaysLabel,
  state = "pending",
  statusLabel,
}: {
  title: string;
  scope: readonly { label: string; value: ReactNode }[];
  children?: ReactNode;
  grantLabel: string;
  denyLabel: string;
  onGrant: () => void;
  onDeny: () => void;
  onAlways?: () => void;
  alwaysLabel?: string;
  state?: "pending" | "session" | "always" | "denied";
  statusLabel?: string;
}) {
  return (
    <AssistantPanel>
      <PanelHeading>
        <span className="flex items-center gap-2">
          <Icon name="key" />
          {title}
        </span>
      </PanelHeading>
      <Facts items={scope} />
      {children}
      {state === "pending" ? (
        <div className="mt-4 flex flex-wrap gap-2">
          {onAlways === undefined ? null : <Button onClick={onAlways}>{alwaysLabel}</Button>}
          <Button onClick={onGrant}>{grantLabel}</Button>
          <Button variant="secondary" onClick={onDeny}>
            {denyLabel}
          </Button>
        </div>
      ) : (
        <output key={state} aria-live="polite" className="motion-safe:animate-assistant-receipt">
          {statusLabel}
        </output>
      )}
    </AssistantPanel>
  );
}

const noComputerSteps: readonly { id: string; x: number; y: number }[] = [];

export function ComputerUse({
  label,
  action,
  image,
  pauseLabel,
  onPause,
  steps = noComputerSteps,
  activeIndex = 0,
}: {
  label: string;
  action: string;
  image: ReactNode;
  pauseLabel: string;
  onPause: () => void;
  steps?: readonly { id: string; x: number; y: number }[];
  activeIndex?: number;
}) {
  return (
    <AssistantPanel>
      <PanelHeading
        actions={
          <Button variant="secondary" onClick={onPause}>
            {pauseLabel}
          </Button>
        }
      >
        {label}
      </PanelHeading>
      <div className="relative overflow-hidden rounded-control">
        {image}
        <svg
          viewBox="0 0 100 100"
          preserveAspectRatio="none"
          aria-hidden="true"
          className="pointer-events-none absolute inset-0 size-full"
        >
          {steps.slice(Math.max(0, activeIndex - 2), activeIndex + 1).map((step) => (
            <circle
              key={step.id}
              cx={Math.max(0, Math.min(100, step.x))}
              cy={Math.max(0, Math.min(100, step.y))}
              r="1"
              className="fill-primary/30 transition-opacity duration-300 motion-reduce:transition-none"
            />
          ))}
          {steps[activeIndex] === undefined ? null : (
            <circle
              cx={Math.max(0, Math.min(100, steps[activeIndex].x))}
              cy={Math.max(0, Math.min(100, steps[activeIndex].y))}
              r="1.8"
              className="fill-primary transition-all duration-500 ease-out motion-reduce:transition-none"
            />
          )}
        </svg>
      </div>
      <output className="mt-3 flex items-center gap-2 text-ui text-text-muted">
        <Icon name="eye" />
        {action}
      </output>
    </AssistantPanel>
  );
}

export function CodeRunner({
  language,
  source,
  runLabel,
  onRun,
  state,
  duration,
  lines,
  statusLabel,
}: {
  language: string;
  source: string;
  runLabel: string;
  onRun: () => void;
  state: "idle" | "running" | "completed" | "failed";
  duration?: string | undefined;
  lines: readonly TerminalLine[];
  statusLabel?: string;
}) {
  const isRunning = state === "running";
  return (
    <AssistantPanel>
      <PanelHeading
        actions={
          <Button onClick={onRun} loading={isRunning} loadingLabel={runLabel}>
            {runLabel}
          </Button>
        }
      >
        {language}
      </PanelHeading>
      <CodeSurface label={language}>{source}</CodeSurface>
      {state === "idle" ? null : (
        <TerminalBlock
          command={language}
          lines={lines}
          statusLabel={statusLabel ?? runLabel}
          running={isRunning}
          duration={duration}
          exitCode={state === "failed" ? 1 : undefined}
        />
      )}
    </AssistantPanel>
  );
}

function useToolGroupMotion() {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const content = ref.current;
    const disclosure = content?.closest("details");
    if (content === null || disclosure === null || disclosure === undefined) {
      return undefined;
    }
    const preference = window.matchMedia("(prefers-reduced-motion: reduce)");
    let animations: Animation[] = [];
    const update = () => {
      for (const animation of animations) {
        animation.cancel();
      }
      animations = [];
      if (!disclosure.open || preference.matches) {
        return;
      }
      animations = [...content.children].map((child, index) =>
        child.animate(
          [
            { opacity: 0, filter: "blur(2px)", transform: "translateY(4px)" },
            { opacity: 1, filter: "blur(0px)", transform: "translateY(0)" },
          ],
          {
            duration: 200,
            delay: Math.min(index + 1, 4) * 40,
            easing: "cubic-bezier(0.32,0.72,0,1)",
            fill: "backwards",
          },
        ),
      );
    };
    update();
    disclosure.addEventListener("toggle", update);
    preference.addEventListener("change", update);
    return () => {
      disclosure.removeEventListener("toggle", update);
      preference.removeEventListener("change", update);
      for (const animation of animations) {
        animation.cancel();
      }
    };
  }, []);
  return ref;
}

export function ToolGroup({
  label,
  children,
  count,
  active = false,
  open,
  defaultOpen,
  onOpenChange,
}: {
  label: string;
  children: ReactNode;
  count?: number;
  active?: boolean;
  open?: boolean;
  defaultOpen?: boolean;
  onOpenChange?: (open: boolean) => void;
}) {
  const content = useToolGroupMotion();
  return (
    <AssistantDisclosure
      open={open}
      {...(defaultOpen === undefined ? {} : { defaultOpen })}
      {...(onOpenChange === undefined ? {} : { onOpenChange })}
      data-motion="tool"
      summary={
        <span className="flex items-center gap-2">
          {active ? <Icon name="loading" className="motion-safe:animate-assistant-spin" /> : null}
          {label}
          {count === undefined ? null : <Badge>{count}</Badge>}
        </span>
      }
    >
      <div ref={content} className="space-y-3">
        {children}
      </div>
    </AssistantDisclosure>
  );
}

/** A controlled configuration request; validation, authorization and persistence belong to the host. */
export function ServerConfigForm({
  url,
  auth,
  urlLabel,
  authLabel,
  submitLabel,
  pending = false,
  error,
  onUrlChange,
  onAuthChange,
  onSubmit,
}: {
  url: string;
  auth: string;
  urlLabel: string;
  authLabel: string;
  submitLabel: string;
  pending?: boolean;
  error?: string;
  onUrlChange: (url: string) => void;
  onAuthChange: (auth: string) => void;
  onSubmit: (configuration: { url: string; auth: string }) => void;
}) {
  const id = useId();
  return (
    <form
      className="space-y-3"
      aria-busy={pending}
      onSubmit={(event) => {
        event.preventDefault();
        if (!pending && url.trim().length > 0) {
          onSubmit({ url: url.trim(), auth });
        }
      }}
    >
      <Field id={`${id}-url`} label={urlLabel}>
        {(control) => (
          <Input
            {...control}
            type="url"
            required
            value={url}
            disabled={pending}
            onChange={(event) => {
              onUrlChange(event.target.value);
            }}
          />
        )}
      </Field>
      <Field id={`${id}-auth`} label={authLabel}>
        {(control) => (
          <Input
            {...control}
            type="password"
            autoComplete="off"
            value={auth}
            disabled={pending}
            onChange={(event) => {
              onAuthChange(event.target.value);
            }}
          />
        )}
      </Field>
      {error === undefined ? null : (
        <p role="alert" className="text-danger">
          {error}
        </p>
      )}
      <Button
        type="submit"
        loading={pending}
        loadingLabel={submitLabel}
        disabled={url.trim().length === 0}
      >
        {submitLabel}
      </Button>
    </form>
  );
}
