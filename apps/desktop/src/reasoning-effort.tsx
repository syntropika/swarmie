import { Button, Slider } from "@swarmie/ui-web";
import { Circle, RotateCcw } from "lucide-react";
import { useLayoutEffect, useRef, useState } from "react";
import type { ReactNode } from "react";

import { startAction } from "~/src/action-boundary";

export function ReasoningEffort({
  choices,
  value,
  disabled,
  onChange,
  renderModel,
  fastControl,
}: {
  choices: { id: string; label: string }[];
  value: string;
  disabled: boolean;
  onChange: (value: string) => Promise<boolean>;
  renderModel?: (effort: string) => ReactNode;
  fastControl?: ReactNode;
}) {
  const index = choices.findIndex((choice) => choice.id === value);
  const choicesKey = choices.map((choice) => choice.id).join("\u0000");
  const key = `${value}\u0000${choicesKey}`;
  const [selection, setSelection] = useState({ key, index, initialValue: value });
  const draft = selection.key === key ? selection.index : index;
  const updateDraft = (nextIndex: number) => {
    setSelection((current) => ({ ...current, key, index: nextIndex }));
  };
  const choose = (next: string) => {
    const nextIndex = choices.findIndex((choice) => choice.id === next);
    if (nextIndex === -1 || next === value) {
      return;
    }
    updateDraft(nextIndex);
    async function commit() {
      if (!(await onChange(next))) {
        updateDraft(index);
      }
    }
    startAction(commit(), () => {
      updateDraft(index);
    });
  };
  const inputRef = useRef<HTMLInputElement>(null);
  const valueLabel = choices[Math.max(0, draft)]?.label ?? "Select effort";
  useLayoutEffect(() => {
    // The vendor formats numeric values; this discrete domain announces the corresponding choice.
    inputRef.current?.setAttribute("aria-valuetext", valueLabel);
  }, [valueLabel]);
  const reset = choices.find((choice) => choice.id === selection.initialValue);
  return (
    <div className="grid gap-3">
      {renderModel ? (
        <>
          <span className="sr-only">Reasoning effort</span>
          <div className="grid grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-2">
            {fastControl ?? <span className="size-7" />}
            {renderModel(choices[draft]?.label ?? "Select effort")}
            <Button
              variant="tertiary"
              size="sm"
              isIconOnly
              aria-label={`Reset effort to ${reset?.label ?? "previous selection"}`}
              title="Restore the effort selected when this menu opened"
              disabled={disabled || !reset || reset.id === value}
              onClick={() => {
                if (reset) {
                  choose(reset.id);
                }
              }}
            >
              <RotateCcw size={16} />
            </Button>
          </div>
        </>
      ) : (
        <div className="flex justify-between text-sm">
          <span>Reasoning effort</span>
          <span>{choices[draft]?.label ?? "Select effort"}</span>
        </div>
      )}
      <Slider
        aria-label="Reasoning effort"
        minValue={0}
        maxValue={Math.max(1, choices.length - 1)}
        step={1}
        value={[Math.max(0, draft)]}
        isDisabled={disabled || choices.length < 2}
        onChange={(nextValue) => {
          updateDraft(Array.isArray(nextValue) ? (nextValue[0] ?? 0) : nextValue);
        }}
        onChangeEnd={(nextValue) => {
          const next = Array.isArray(nextValue) ? (nextValue[0] ?? 0) : nextValue;
          const choice = choices.at(next);
          if (choice !== undefined) {
            choose(choice.id);
          }
        }}
      >
        <Slider.Track>
          <Slider.Fill />
          <div
            className="pointer-events-none absolute inset-0 flex items-center justify-between px-3"
            aria-hidden="true"
          >
            {choices.map((choice) => (
              <span key={choice.id} className="opacity-45">
                <Circle size={4} fill="currentColor" />
              </span>
            ))}
          </div>
          <Slider.Thumb inputRef={inputRef} />
        </Slider.Track>
      </Slider>
    </div>
  );
}
