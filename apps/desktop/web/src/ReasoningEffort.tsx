import { Slider } from "@/components/ui/slider";
import { useEffect, useState, type ReactNode } from "react";
import { Circle, RotateCcw } from "lucide-react";
import { Button } from "@/components/ui/button";

export function ReasoningEffort({ choices, value, disabled, onChange, modelControl, fastControl }: {
  choices: { id: string; label: string }[];
  value: string;
  disabled: boolean;
  onChange: (value: string) => Promise<boolean>;
  modelControl?: (effort: string) => ReactNode;
  fastControl?: ReactNode;
}) {
  const index = choices.findIndex((choice) => choice.id === value);
  const [draft, setDraft] = useState(index);
  const [initialValue] = useState(value);
  const choicesKey = choices.map((choice) => choice.id).join("\u0000");
  useEffect(() => { setDraft(index); }, [index, choicesKey]);
  const choose = (next: string) => {
    const nextIndex = choices.findIndex((choice) => choice.id === next);
    if (nextIndex < 0 || next === value) return;
    setDraft(nextIndex);
    void onChange(next).then((success) => { if (!success) setDraft(index); });
  };
  const reset = choices.find((choice) => choice.id === initialValue);
  return <div className="grid gap-3">
    {modelControl ? <>
      <span className="sr-only">Reasoning effort</span>
      <div className="grid grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-2">
        {fastControl ?? <span className="size-7" />}
        {modelControl(choices[draft]?.label || "Select effort")}
        <Button variant="ghost" size="icon-sm" aria-label={`Reset effort to ${reset?.label ?? "previous selection"}`}
          title="Restore the effort selected when this menu opened" disabled={disabled || !reset || reset.id === value}
          onClick={() => { if (reset) choose(reset.id); }}><RotateCcw size={16} /></Button>
      </div>
    </> : <div className="flex justify-between text-sm"><span>Reasoning effort</span><span>{choices[draft]?.label || "Select effort"}</span></div>}
    <Slider label="Reasoning effort" valueText={choices[Math.max(0, draft)]?.label || "Select effort"} min={0} max={Math.max(1, choices.length - 1)} step={1} value={[Math.max(0, draft)]} disabled={disabled || choices.length < 2}
      onValueChange={([next]) => setDraft(next)}
      onValueCommit={([next]) => {
        const choice = choices[next];
        if (choice && choice.id !== value) void onChange(choice.id).then((success) => { if (!success) setDraft(index); });
      }}>
        <div className="pointer-events-none absolute inset-0 flex items-center justify-between px-3" aria-hidden="true">{choices.map((choice) => <span key={choice.id} className="opacity-45"><Circle size={4} fill="currentColor" /></span>)}</div>
    </Slider>
  </div>;
}
