import { useId } from "react";

import { Description } from "./description";
import { Label } from "./label";
import { Slider } from "./slider";

export type ReasoningLevel<T extends string = string> = { id: T; label: string };

export type ReasoningEffortProps<T extends string = string> = {
  levels: readonly ReasoningLevel<T>[];
  value: T;
  onChange: (value: T) => void;
  model?: string | undefined;
  label?: string;
  isDisabled?: boolean;
};

/** Map consumer-owned reasoning levels onto the vendor's discrete slider. */
export function ReasoningEffort<T extends string>({
  levels,
  value,
  onChange,
  model,
  label = "Thinking",
  isDisabled = false,
}: ReasoningEffortProps<T>) {
  const descriptionId = useId();
  const hasModel = model !== undefined && model !== "";
  const index = levels.findIndex((level) => level.id === value);
  const selected = levels[index];
  const currentLabel = selected?.label ?? "Unavailable";
  const disabled = isDisabled || levels.length < 2 || selected === undefined;
  if (levels.length === 0) {
    return (
      <div className="grid gap-1">
        <Label>{label}</Label>
        <output>{currentLabel}</output>
        {hasModel ? <Description>{model}</Description> : null}
      </div>
    );
  }
  return (
    <div className="grid gap-1">
      <Slider
        aria-describedby={descriptionId}
        minValue={0}
        maxValue={Math.max(1, levels.length - 1)}
        step={1}
        value={Math.max(0, index)}
        isDisabled={disabled}
        onChange={(next) => {
          const position = typeof next === "number" ? next : next[0];
          const level = position === undefined ? undefined : levels[position];
          if (level !== undefined) {
            onChange(level.id);
          }
        }}
      >
        <Label>
          {label}
          <span className="sr-only">: {currentLabel}</span>
        </Label>
        <Slider.Output>{currentLabel}</Slider.Output>
        <Slider.Track>
          <Slider.Fill />
          <Slider.Thumb />
        </Slider.Track>
      </Slider>
      <Description id={descriptionId}>
        {hasModel ? `${model}: ${currentLabel}` : currentLabel}
      </Description>
    </div>
  );
}
