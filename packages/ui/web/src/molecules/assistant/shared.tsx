import type { ComponentPropsWithRef, ReactNode } from "react";
import { useState } from "react";

import { Badge } from "../../atoms/badge";
import { Button } from "../../atoms/button";
import { Icon } from "../../atoms/icon";
import type { IconName } from "../../atoms/icon";
import { TooltipContent, TooltipProvider, TooltipRoot, TooltipTrigger } from "../../atoms/tooltip";
import { cn } from "../../classes";

/** Presentation lifecycle; consumers own execution and choose the label. */
export type ExecutionState = "idle" | "running" | "waiting" | "completed" | "failed" | "cancelled";

const stateTones = {
  idle: "neutral",
  running: "agent",
  waiting: "attention",
  completed: "success",
  failed: "danger",
  cancelled: "neutral",
} as const;

export function ExecutionStatus({
  state,
  children,
}: {
  state: ExecutionState;
  children: ReactNode;
}) {
  return <Badge tone={stateTones[state]}>{children}</Badge>;
}

/** A compact panel for inspectable content, inheriting Swarmie's existing tokens. */
export function AssistantPanel({ className, ...props }: ComponentPropsWithRef<"section">) {
  return (
    <section
      {...props}
      className={cn(
        "@container min-w-0 rounded-panel border border-transparent bg-surface p-3 text-ui leading-ui text-ink",
        className,
      )}
    />
  );
}

export function PanelHeading({ children, actions }: { children: ReactNode; actions?: ReactNode }) {
  return (
    <div className="mb-3 flex flex-wrap items-center justify-between gap-3">
      <div className="min-w-0 text-ui leading-ui font-medium">{children}</div>
      {actions}
    </div>
  );
}

/** Native disclosure preserves keyboard, open state, and accessibility semantics. */
export function AssistantDisclosure({
  summary,
  children,
  className,
  open,
  defaultOpen = false,
  onOpenChange,
  onToggle,
  ...props
}: Omit<ComponentPropsWithRef<"details">, "children"> & {
  summary: ReactNode;
  children: ReactNode;
  defaultOpen?: boolean;
  onOpenChange?: (open: boolean) => void;
}) {
  const [uncontrolledOpen, setUncontrolledOpen] = useState(defaultOpen);
  const expanded = open ?? uncontrolledOpen;
  return (
    <details
      {...props}
      open={expanded}
      data-motion="tool"
      className={cn(
        "assistant-disclosure group min-w-0 rounded-panel border border-transparent bg-surface text-ui text-ink",
        className,
      )}
      onToggle={(event) => {
        if (open === undefined) {
          const nextOpen = event.currentTarget.open;
          setUncontrolledOpen(nextOpen);
          onOpenChange?.(nextOpen);
        }
        onToggle?.(event);
      }}
    >
      <summary
        className="flex min-h-control cursor-pointer list-none items-center gap-3 rounded-pill px-3 py-2 text-ui leading-ui font-medium hover:bg-surface-hover focus-visible:outline-2 focus-visible:outline-primary pointer-coarse:min-h-control-touch"
        onClick={(event) => {
          if (open !== undefined) {
            event.preventDefault();
            onOpenChange?.(!open);
          }
        }}
      >
        <Icon
          name="chevron"
          className="size-4 transition-transform duration-200 ease-assistant-disclosure group-open:rotate-180 motion-reduce:transition-none"
        />
        <span className="min-w-0 flex-1">{summary}</span>
      </summary>
      <div className="p-3 pt-1">{children}</div>
    </details>
  );
}

/** Accessible icon action with a hover/focus tooltip; labels belong to the caller. */
export function TooltipIconButton({
  label,
  icon,
  side = "bottom",
  ...props
}: Omit<ComponentPropsWithRef<"button">, "children" | "title"> & {
  label: string;
  icon: IconName;
  side?: "top" | "bottom" | "left" | "right";
}) {
  return (
    <TooltipProvider>
      <TooltipRoot>
        <TooltipTrigger asChild>
          <Button {...props} variant="ghost" size="icon-sm" aria-label={label}>
            <Icon name={icon} />
          </Button>
        </TooltipTrigger>
        <TooltipContent side={side}>{label}</TooltipContent>
      </TooltipRoot>
    </TooltipProvider>
  );
}

/** Native progress avoids inline style geometry and exposes a numerical value. */
export function AssistantProgress({
  value,
  max = 100,
  label,
  className,
  ...props
}: Omit<ComponentPropsWithRef<"progress">, "value" | "max" | "children"> & {
  value: number;
  max?: number;
  label: string;
  className?: string;
}) {
  const maximum = Number.isFinite(max) && max > 0 ? max : 1;
  const current = Number.isFinite(value) ? Math.min(Math.max(value, 0), maximum) : 0;
  return (
    <progress
      {...props}
      aria-label={label}
      max={maximum}
      value={current}
      className={cn(
        "[&::-moz-progress-bar]:transition-width [&::-webkit-progress-value]:transition-width h-2 w-full overflow-hidden rounded-full accent-primary [&::-moz-progress-bar]:bg-primary [&::-moz-progress-bar]:duration-500 [&::-moz-progress-bar]:ease-out motion-reduce:[&::-moz-progress-bar]:transition-none [&::-webkit-progress-bar]:bg-surface-muted [&::-webkit-progress-value]:bg-primary [&::-webkit-progress-value]:duration-500 [&::-webkit-progress-value]:ease-out motion-reduce:[&::-webkit-progress-value]:transition-none",
        className,
      )}
    />
  );
}

export function CodeSurface({ children, label }: { children: ReactNode; label: string }) {
  return (
    <pre
      aria-label={label}
      // oxlint-disable-next-line jsx-a11y/no-noninteractive-tabindex -- Keyboard users need focus to scroll this bounded content region.
      tabIndex={0}
      className="overflow-x-auto rounded-control bg-inverse p-3 font-mono text-code leading-code text-inverse-text focus-visible:outline-2 focus-visible:outline-primary"
    >
      <code>{children}</code>
    </pre>
  );
}

export type LabeledValue = { label: string; value: ReactNode };

export function Facts({ items }: { items: readonly LabeledValue[] }) {
  return (
    <dl className="grid grid-cols-[minmax(0,1fr)_minmax(0,2fr)] gap-x-4 gap-y-3 text-ui">
      {items.map(({ label, value }) => (
        <div key={label} className="contents">
          <dt className="text-text-muted">{label}</dt>
          <dd className="min-w-0 font-medium break-words">{value}</dd>
        </div>
      ))}
    </dl>
  );
}
