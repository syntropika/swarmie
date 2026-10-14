import { Badge } from "../../atoms/badge";
import { Button } from "../../atoms/button";
import { TooltipProvider, TooltipRoot, TooltipTrigger, TooltipContent } from "../../atoms/tooltip";
import { cn } from "../../classes";
import { AlertContent, AlertDescription, AlertRoot, AlertTitle } from "../alert";
import { AssistantPanel, AssistantProgress, PanelHeading } from "./shared";

export type TraceSpan = {
  id: string;
  label: string;
  start: number;
  duration: number;
  depth: number;
  detail: string;
  state?: "running" | "completed" | "failed";
  statusLabel?: string;
};

const defaultFormatTime = (value: number) => `${value} ms`;

/** The visual time axis and adjacent span data describe the same run. */
export function TraceWaterfall({
  label,
  spans,
  duration,
  formatTime = defaultFormatTime,
}: {
  label: string;
  spans: readonly TraceSpan[];
  duration: number;
  formatTime?: (value: number) => string;
}) {
  const total = Math.max(1, duration);
  return (
    <AssistantPanel>
      <PanelHeading>{label}</PanelHeading>
      <svg
        viewBox={`0 0 600 ${Math.max(60, spans.length * 32 + 55)}`}
        // oxlint-disable-next-line jsx-a11y/prefer-tag-over-role -- Inline SVG geometry needs graphic semantics; the title supplies its accessible description.
        role="img"
        aria-label={label}
        className="w-full"
      >
        <title>{label}</title>
        {spans.map((span, index) => (
          <g key={span.id} className="motion-safe:animate-assistant-slide-left">
            <text x={span.depth * 12} y={index * 32 + 20} className="fill-ink text-xs">
              {span.label}
            </text>
            <rect
              x={220 + (span.start / total) * 360}
              y={index * 32 + 8}
              width={Math.max(2, (span.duration / total) * 360)}
              height="16"
              rx="4"
              className={cn(
                "transition-all duration-500 ease-out motion-reduce:transition-none",
                span.state === "failed" ? "fill-danger" : "fill-primary",
                span.state === "running" && "motion-safe:animate-pulse",
              )}
            >
              <title>
                {span.statusLabel}: {span.detail}
              </title>
            </rect>
          </g>
        ))}
        {[0, 0.25, 0.5, 0.75, 1].map((fraction) => (
          <text
            key={fraction}
            x={220 + fraction * 360}
            y={spans.length * 32 + 30}
            textAnchor={fraction === 1 ? "end" : "start"}
            className="fill-text-muted text-xs"
          >
            {formatTime(duration * fraction)}
          </text>
        ))}
      </svg>
      <ul className="space-y-1 text-xs text-text-muted">
        {spans.map((span) => (
          <li key={span.id}>
            {span.label}: {span.statusLabel} {span.detail}
          </li>
        ))}
      </ul>
    </AssistantPanel>
  );
}

export type ModelCost = {
  id: string;
  model: string;
  amount: number;
  tokens: string;
  inputTokens?: number;
  outputTokens?: number;
};

export function CostMeter({
  label,
  costs,
  budget,
  locale = "en",
  currency = "USD",
  sessionAmount,
  sessionLabel,
  inputLabel = "Input",
  outputLabel = "Output",
}: {
  label: string;
  costs: readonly ModelCost[];
  budget?: number;
  locale?: string;
  currency?: string;
  sessionAmount?: number;
  sessionLabel?: string;
  inputLabel?: string;
  outputLabel?: string;
}) {
  const format = new Intl.NumberFormat(locale, {
    style: "currency",
    currency,
    maximumFractionDigits: 4,
  });
  const total = costs.reduce((sum, entry) => sum + entry.amount, 0);
  return (
    <AssistantPanel>
      <PanelHeading actions={<Badge>{format.format(total)}</Badge>}>{label}</PanelHeading>
      {sessionAmount === undefined ? null : (
        <p className="text-xs text-text-muted">
          {sessionLabel}: {format.format(sessionAmount)}
        </p>
      )}
      <svg
        viewBox="0 0 100 4"
        aria-hidden="true"
        className="my-2 h-2 w-full overflow-hidden rounded-pill"
      >
        {costs.map((entry, index) => (
          <rect
            key={entry.id}
            x={
              total > 0
                ? (costs.slice(0, index).reduce((sum, cost) => sum + cost.amount, 0) / total) * 100
                : 0
            }
            width={total > 0 ? (entry.amount / total) * 100 : 0}
            height="4"
            className={cn(
              "transition-all duration-500 motion-reduce:transition-none",
              index % 2 === 0 ? "fill-primary" : "fill-primary/50",
            )}
          />
        ))}
      </svg>
      {budget === undefined ? null : (
        <AssistantProgress label={label} value={total} max={Math.max(0.0001, budget)} />
      )}
      <ul className="mt-3 space-y-3">
        {costs.map((entry) => (
          <li key={entry.id} className="flex flex-wrap justify-between gap-2 text-ui">
            <span>
              {entry.model}
              <span className="ml-2 text-xs text-text-muted">{entry.tokens}</span>
              {entry.inputTokens === undefined ? null : (
                <span className="ml-2 text-xs text-text-muted">
                  {inputLabel}: {entry.inputTokens.toLocaleString(locale)} · {outputLabel}:{" "}
                  {(entry.outputTokens ?? 0).toLocaleString(locale)}
                </span>
              )}
            </span>
            <span className="tabular-nums">{format.format(entry.amount)}</span>
          </li>
        ))}
      </ul>
    </AssistantPanel>
  );
}

export function QuotaBanner({
  title,
  remaining,
  maximum,
  resetLabel,
  actionLabel,
  onAction,
  unit,
  usageLabel,
}: {
  title: string;
  remaining: number;
  maximum: number;
  resetLabel: string;
  actionLabel: string;
  onAction: () => void;
  unit?: string;
  usageLabel?: string;
}) {
  return (
    <AlertRoot
      tone="info"
      className={
        maximum > 0 && (maximum - remaining) / maximum >= 0.9
          ? "bg-attention-soft text-attention"
          : undefined
      }
    >
      <AlertContent>
        <AlertTitle>{title}</AlertTitle>
        <AlertDescription>{resetLabel}</AlertDescription>
        <p className="text-xs">
          {Math.max(0, maximum - remaining)} / {maximum} {unit}
        </p>
        <AssistantProgress
          label={usageLabel ?? title}
          value={Math.max(0, maximum - remaining)}
          max={Math.max(1, maximum)}
        />
        <Button className="mt-3" variant="secondary" onClick={onAction}>
          {actionLabel}
        </Button>
      </AlertContent>
    </AlertRoot>
  );
}

export function MessageTiming({
  label,
  firstToken,
  total,
  speed,
  chunks,
  completed = true,
}: {
  label: string;
  firstToken: string;
  total: string;
  speed: string;
  chunks?: string;
  completed?: boolean;
}) {
  if (!completed) {
    return null;
  }
  return (
    <TooltipProvider>
      <TooltipRoot>
        <TooltipTrigger asChild>
          <Button variant="ghost" size="sm" aria-label={label}>
            {total}
          </Button>
        </TooltipTrigger>
        <TooltipContent>
          <dl aria-label={label} className="flex flex-wrap gap-x-5 gap-y-2 text-xs">
            <div>
              <dt>First token</dt>
              <dd className="font-medium tabular-nums">{firstToken}</dd>
            </div>
            <div>
              <dt>Total time</dt>
              <dd className="font-medium tabular-nums">{total}</dd>
            </div>
            <div>
              <dt>Speed</dt>
              <dd className="font-medium tabular-nums">{speed}</dd>
            </div>
            {chunks === undefined ? null : (
              <div>
                <dt>Chunks</dt>
                <dd>{chunks}</dd>
              </div>
            )}
          </dl>
        </TooltipContent>
      </TooltipRoot>
    </TooltipProvider>
  );
}
