import { Select, ListBox } from "@swarmie/ui-web";
import { useState } from "react";

import { startAction } from "~/src/action-boundary";
import { hasText } from "~/src/value-presence";

export function ModelPicker({
  choices,
  value,
  label,
  effort,
  disabled,
  onChange,
}: {
  choices: { id: string; label: string }[];
  value: string;
  label: string;
  effort?: string;
  disabled: boolean;
  onChange: (value: string) => Promise<boolean>;
}) {
  const [open, setOpen] = useState(false);
  const choose = async (next: string) => {
    if (!(await onChange(next))) {
      setOpen(true);
    }
  };
  return (
    <Select
      isOpen={open}
      onOpenChange={setOpen}
      value={value || null}
      isDisabled={disabled || choices.length === 0}
      aria-label={`Choose model: ${label}`}
      onChange={(next) => {
        if (typeof next === "string" && next !== value) {
          startAction(choose(next), () => {
            setOpen(true);
          });
        }
      }}
    >
      <Select.Trigger>
        {hasText(effort) && <span className="capitalize">{effort}</span>}
        <Select.Value>{label}</Select.Value>
        <Select.Indicator />
      </Select.Trigger>
      <Select.Popover placement="top end" className="w-80">
        <ListBox aria-label="Available models">
          {choices.map((choice) => (
            <ListBox.Item key={choice.id} id={choice.id} textValue={choice.label}>
              {choice.label}
              <ListBox.ItemIndicator />
            </ListBox.Item>
          ))}
        </ListBox>
      </Select.Popover>
    </Select>
  );
}
