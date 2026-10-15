import * as Primitive from "@radix-ui/react-select";
import { cn } from "cn";
import { Check, ChevronRight } from "lucide-react";
import { useState } from "react";

import { startAction } from "~/web/src/action-boundary";
import { Button } from "~/web/src/components/ui/button";
import { floatingClasses, itemClasses } from "~/web/src/components/ui/floating";
import { hasText } from "~/web/src/value-presence";

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
    <Primitive.Root
      open={open}
      onOpenChange={setOpen}
      value={value}
      onValueChange={(next) => {
        if (next !== value) {
          startAction(choose(next), () => {
            setOpen(true);
          });
        }
      }}
    >
      <Primitive.Trigger asChild>
        <Button
          variant="secondary"
          size={hasText(effort) ? "lg" : "sm"}
          className="max-w-full min-w-0 justify-self-center"
          aria-label={`Choose model: ${label}`}
          disabled={disabled || !choices.length}
        >
          <span className="flex min-w-0 flex-col items-center">
            {hasText(effort) && <span className="text-focus capitalize">{effort}</span>}
            <span className="text-muted flex max-w-full items-center gap-1 text-xs">
              {label}
              <ChevronRight size={12} />
            </span>
          </span>
        </Button>
      </Primitive.Trigger>
      <Primitive.Portal>
        <Primitive.Content
          position="popper"
          side="top"
          align="end"
          sideOffset={8}
          collisionPadding={20}
          aria-label="Available models"
          className={cn(
            floatingClasses,
            "max-h-[var(--radix-select-content-available-height)] w-80 p-1.5",
          )}
        >
          <div className="text-muted px-2.5 py-1.5 text-sm">Select model</div>
          <Primitive.Viewport className="max-h-65 overflow-y-auto">
            {choices.map((choice) => (
              <Primitive.Item
                key={choice.id}
                value={choice.id}
                textValue={choice.label}
                disabled={disabled}
                className={itemClasses}
              >
                <Primitive.ItemText>{choice.label}</Primitive.ItemText>
                <Primitive.ItemIndicator className="ml-auto">
                  <Check size={14} />
                </Primitive.ItemIndicator>
              </Primitive.Item>
            ))}
          </Primitive.Viewport>
        </Primitive.Content>
      </Primitive.Portal>
    </Primitive.Root>
  );
}
