import { useId } from "react";

import { SelectContent, SelectItem, SelectRoot, SelectTrigger, SelectValue } from "../atoms/select";
import { cn } from "../classes";
import { isThemePreference } from "../theme-preference";
import type { ThemePreference } from "../theme-preference";

export type ThemePreferenceControlProps = {
  value: ThemePreference;
  onValueChange: (value: ThemePreference) => void;
  label: string;
  lightLabel: string;
  darkLabel: string;
  systemLabel: string;
  tone?: "surface" | "footer";
  layout?: "stacked" | "row";
};

/** A persistent label and compact picker, with copy and preference state owned by its consumer. */
export function ThemePreferenceControl({
  value,
  onValueChange,
  label,
  lightLabel,
  darkLabel,
  systemLabel,
  tone = "surface",
  layout = "stacked",
}: ThemePreferenceControlProps) {
  const id = useId();
  const labels = { light: lightLabel, dark: darkLabel, system: systemLabel };
  return (
    <div
      className={cn(
        "flex w-full min-w-0",
        layout === "row" ? "flex-wrap items-center justify-between gap-3" : "flex-col gap-2",
        tone === "footer" ? "[&_button:focus-visible]:outline-footer-text" : "text-ink",
      )}
    >
      <label htmlFor={id} className="text-ui leading-snug font-medium">
        {label}
      </label>
      <div className={layout === "row" ? "max-w-52 min-w-32 flex-1" : undefined}>
        <SelectRoot
          value={value}
          onValueChange={(next) => {
            if (isThemePreference(next)) {
              onValueChange(next);
            }
          }}
        >
          <SelectTrigger
            id={id}
            controlSize="compact"
            tone={tone === "footer" ? "inverse" : "default"}
            className="w-full"
          >
            <SelectValue>{labels[value]}</SelectValue>
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="light">{lightLabel}</SelectItem>
            <SelectItem value="dark">{darkLabel}</SelectItem>
            <SelectItem value="system">{systemLabel}</SelectItem>
          </SelectContent>
        </SelectRoot>
      </div>
    </div>
  );
}
