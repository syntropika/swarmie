import { useState } from "react";

export type ActionDrafts = {
  branch: string | null;
  remote: string;
  pushDestination: string;
  commitMessage: string;
  reviewedIndex: string | null;
  worktreeDestination: string;
  managedRoot: string | null;
  startingCommit: string | null;
  includedPaths: string[] | null;
};

/** Drafts belong to the captured checkout, independently of refreshes and panel placement. */
export function useActionDrafts() {
  const [drafts, setDrafts] = useState<ActionDrafts>({
    branch: null,
    remote: "",
    pushDestination: "",
    commitMessage: "",
    reviewedIndex: null,
    worktreeDestination: "",
    managedRoot: null,
    startingCommit: null,
    includedPaths: null,
  });
  function updateDraft(update: Partial<ActionDrafts>) {
    setDrafts((current) => ({ ...current, ...update }));
  }
  return { drafts, updateDraft };
}

export type DraftProps = ReturnType<typeof useActionDrafts>;
