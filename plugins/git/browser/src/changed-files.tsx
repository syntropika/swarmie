import { Button, VirtualList } from "@swarmie/ui-web";
import { FileCode2, Minus, Plus } from "lucide-react";
import { useMemo } from "react";

import type { Checkout } from "./checkout-data";
import { changeGroups } from "./file-selection";
import type { GitOperation, StatusEntry } from "./protocol";
import type { Selection } from "./workspace";

function status(entry: StatusEntry, staged: boolean) {
  const value = staged ? entry.index : entry.worktree;
  if (entry.kind === "untracked") {
    return { value: "U", tone: "added", label: "Untracked" };
  }
  if (entry.kind === "unmerged" || value === "D") {
    return { value, tone: "removed", label: value };
  }
  if (value === "A") {
    return { value, tone: "added", label: value };
  }
  return { value, tone: "modified", label: value };
}

type ChangeRow =
  | { kind: "group"; title: string; count: number }
  | { kind: "file"; entry: StatusEntry; staged: boolean };
function changeRowKey(row: ChangeRow) {
  return row.kind === "group" ? `group:${row.title}` : JSON.stringify([row.staged, row.entry.path]);
}

export function ChangedFiles({
  checkout,
  selection,
  disabled,
  select,
  mutate,
  onError,
  filter,
  compact,
}: {
  checkout: Checkout;
  selection: Selection | null;
  disabled: boolean;
  select: (selection: Selection) => void;
  mutate: (operation: GitOperation) => Promise<boolean>;
  onError: (error: unknown) => void;
  filter: "all" | "staged" | "unstaged";
  compact: boolean;
}) {
  const rows = useMemo(
    () =>
      changeGroups(checkout.status.entries, filter).flatMap((group): ChangeRow[] => [
        { kind: "group", title: group.title, count: group.entries.length },
        ...group.entries.map((entry): ChangeRow => ({ kind: "file", entry, staged: group.staged })),
      ]),
    [checkout.status.entries, filter],
  );
  return (
    <VirtualList
      items={rows}
      getItemKey={changeRowKey}
      label="Changed files"
      estimateSize={compact ? 56 : 36}
      className="h-full"
      renderItem={(row) => {
        if (row.kind === "group") {
          return (
            <h3 className="flex justify-between px-4 pt-4 pb-2 text-xs font-medium text-muted">
              {row.title}
              <span className="tabular-nums">{row.count}</span>
            </h3>
          );
        }
        const { entry } = row;
        return (
          <div
            data-git-file-row=""
            className={`flex items-center gap-1 pr-2 ${selection?.path === entry.path && selection.staged === row.staged ? "bg-brand-accent/15" : ""}`}
          >
            <button
              type="button"
              className="flex min-w-0 flex-1 items-center gap-2 px-4 py-2 text-left text-xs hover:bg-default focus-visible:outline-2 focus-visible:-outline-offset-2"
              onClick={() => {
                select({
                  path: entry.path,
                  staged: row.staged,
                  source: entry.kind === "untracked" ? "file" : "diff",
                });
              }}
            >
              <FileCode2 size={14} className="shrink-0 text-muted" />
              <span className="block flex-1 truncate" title={entry.path}>
                {entry.path}
              </span>
              <span
                className="shrink-0 font-mono"
                data-git-status={status(entry, row.staged).tone}
                title={status(entry, row.staged).label}
              >
                {status(entry, row.staged).value}
              </span>
              {entry.kind === "unmerged" ? (
                <span className="text-danger">Conflict — resolve before staging</span>
              ) : null}
            </button>
            <Button
              size="sm"
              variant="tertiary"
              aria-label={`${row.staged ? "Unstage" : "Stage"} ${entry.path}`}
              isDisabled={disabled || entry.kind === "unmerged"}
              onPress={() => {
                mutate({
                  kind: row.staged ? "unstage" : "stage",
                  paths:
                    entry.original_path === null ? [entry.path] : [entry.original_path, entry.path],
                }).catch(onError);
              }}
            >
              {row.staged ? (
                <Minus size={14} data-git-status="modified" />
              ) : (
                <Plus size={14} data-git-status="added" />
              )}
            </Button>
          </div>
        );
      }}
    />
  );
}
