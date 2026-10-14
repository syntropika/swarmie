import type { ComponentPropsWithRef } from "react";

import { Button } from "../../atoms/button";
import { cn } from "../../classes";
import { AssistantProgress } from "./shared";

export type ReasoningEffortOption = { value: string; label: string; budget: number };

export type ReasoningEffortProps = Omit<ComponentPropsWithRef<"div">, "onChange"> & {
  label: string;
  value: string;
  options: readonly ReasoningEffortOption[];
  spent: number;
  usageLabel: string;
  onChange?: (value: string) => void;
  formatNumber?: (value: number) => string;
};

const formatNumberDefault = (value: number) => new Intl.NumberFormat("en-US").format(value);

/** Assistant-ui's selectable or read-only budget segments, independent of the AICSS slider. */
export function ReasoningEffort({
  label,
  value,
  options,
  spent,
  usageLabel,
  onChange,
  formatNumber = formatNumberDefault,
  className,
  ...props
}: ReasoningEffortProps) {
  const budget = options.find((option) => option.value === value)?.budget ?? 0;
  return (
    <div {...props} className={cn("flex w-full max-w-sm flex-col gap-2.5", className)}>
      <div className="flex items-baseline justify-between gap-2 text-ui leading-ui">
        <span className="font-medium">{label}</span>
        <span className="font-mono text-code text-text-muted tabular-nums">
          {formatNumber(spent)} / {formatNumber(budget)}
        </span>
      </div>
      <div className="flex gap-0.5 rounded-pill bg-surface-muted p-0.5">
        {options.map((option) =>
          onChange ? (
            <Button
              key={option.value}
              variant="choice"
              size="menu"
              selected={option.value === value}
              aria-pressed={option.value === value}
              className="flex-1"
              onClick={() => {
                onChange(option.value);
              }}
            >
              {option.label}
            </Button>
          ) : (
            <span
              key={option.value}
              aria-current={option.value === value ? "true" : undefined}
              className={cn(
                "flex-1 rounded-pill px-2 py-1 text-center text-ui font-medium",
                option.value === value ? "bg-surface text-ink" : "text-text-muted",
              )}
            >
              {option.label}
            </span>
          ),
        )}
      </div>
      <AssistantProgress
        max={Math.max(1, budget)}
        value={budget > 0 ? spent : 0}
        label={usageLabel}
        className="h-1 [&::-webkit-progress-value]:transition-all [&::-webkit-progress-value]:duration-500 motion-reduce:[&::-webkit-progress-value]:transition-none"
      />
    </div>
  );
}
