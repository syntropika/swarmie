import type { StatusEntry } from "./protocol";
import type { ChangesFilter } from "./view-controls";

export type Selection = { path: string; staged: boolean; source: "file" | "diff" };

export function changeGroups(entries: StatusEntry[], filter: ChangesFilter) {
  return [
    {
      title: "Staged changes",
      staged: true,
      entries: entries.filter(
        (entry) => entry.kind === "tracked" && entry.index !== "." && entry.index !== " ",
      ),
    },
    {
      title: "Changes",
      staged: false,
      entries: entries.filter(
        (entry) =>
          entry.kind === "unmerged" ||
          (entry.kind === "tracked" && entry.worktree !== "." && entry.worktree !== " "),
      ),
    },
    {
      title: "Untracked",
      staged: false,
      entries: entries.filter((entry) => entry.kind === "untracked"),
    },
  ].filter((group) => filter === "all" || group.staged === (filter === "staged"));
}

/** Keep the file under review when staging moves it to the other side of the index. */
export function compactSelection(
  entries: StatusEntry[],
  filter: ChangesFilter,
  selected: Selection | null,
): Selection | null {
  const choices = changeGroups(entries, filter).flatMap((group) =>
    group.entries.map((entry): Selection => ({
      path: entry.path,
      staged: group.staged,
      source: entry.kind === "untracked" ? "file" : "diff",
    })),
  );
  return (
    choices.find((entry) => entry.path === selected?.path && entry.staged === selected.staged) ??
    choices.find((entry) => entry.path === selected?.path) ??
    choices.find((entry) => !entry.staged) ??
    choices[0] ??
    null
  );
}
