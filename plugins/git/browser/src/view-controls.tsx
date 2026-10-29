import { ToggleButton, ToggleButtonGroup } from "@swarmie/ui-web";

import type { DiffMode } from "./protocol";

export type ChangesFilter = "all" | "staged" | "unstaged";

export function DiffLayout({
  value,
  onChange,
}: {
  value: DiffMode;
  onChange: (value: DiffMode) => void;
}) {
  return (
    <ToggleButtonGroup
      data-git-segments=""
      aria-label="Diff layout"
      selectionMode="single"
      disallowEmptySelection
      selectedKeys={[value]}
      size="sm"
      onSelectionChange={(keys) => {
        const [layout] = keys;
        if (layout === "unified" || layout === "split") {
          onChange(layout);
        }
      }}
    >
      <ToggleButton id="unified">Unified</ToggleButton>
      <ToggleButton id="split">Split</ToggleButton>
    </ToggleButtonGroup>
  );
}

const filterLabels = { all: "All", staged: "Staged", unstaged: "Unstaged" };

export function ChangeFilter({
  value,
  onChange,
}: {
  value: ChangesFilter;
  onChange: (value: ChangesFilter) => void;
}) {
  return (
    <ToggleButtonGroup
      data-git-segments=""
      aria-label="Changes to review"
      selectionMode="single"
      disallowEmptySelection
      selectedKeys={[value]}
      size="sm"
      onSelectionChange={(keys) => {
        const [filter] = keys;
        if (filter === "all" || filter === "staged" || filter === "unstaged") {
          onChange(filter);
        }
      }}
    >
      {(["all", "staged", "unstaged"] as const).map((filter) => (
        <ToggleButton key={filter} id={filter}>
          {filterLabels[filter]}
        </ToggleButton>
      ))}
    </ToggleButtonGroup>
  );
}
