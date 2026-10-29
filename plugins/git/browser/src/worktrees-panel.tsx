import { Button, Input, Label, TextField, VirtualList } from "@swarmie/ui-web";
import { useEffect, useMemo, useState } from "react";

import type { DraftProps } from "./drafts";
import { readArchives } from "./operation-catalog";
import type { ArchiveReceipt, StatusEntry, UnavailableEntry } from "./protocol";
import type { useCheckout } from "./use-checkout";

type State = ReturnType<typeof useCheckout>;
function candidateKey(entry: StatusEntry) {
  return JSON.stringify([entry.kind, entry.path]);
}
function archiveKey(entry: ArchiveReceipt) {
  return entry.archive_id;
}
function unavailableKey(entry: UnavailableEntry) {
  return entry.entry_id;
}
export function WorktreesPanel({
  state,
  projectId,
  onWorktreeCreated,
  drafts,
  updateDraft,
}: DraftProps & {
  state: State;
  projectId: string | undefined;
  onWorktreeCreated: ((path: string) => void) | undefined;
}) {
  const destination = drafts.worktreeDestination;
  const root = drafts.managedRoot ?? state.settings?.managed_root ?? "";
  const commit = drafts.startingCommit ?? state.checkout?.target.head ?? "";
  const defaults = useMemo(() => {
    if (state.settings?.include_local_changes !== true) {
      return [];
    }
    return (
      state.checkout?.status.entries
        .filter((entry) => entry.kind === "tracked" || entry.kind === "untracked")
        .flatMap((entry) =>
          entry.original_path === null ? [entry.path] : [entry.path, entry.original_path],
        ) ?? []
    );
  }, [state.settings?.include_local_changes, state.checkout?.status.entries]);
  const included = drafts.includedPaths ?? defaults;
  const [message, setMessage] = useState<string | null>(null);
  const { checkout } = state;
  const candidates = useMemo(
    () =>
      checkout?.status.entries.filter(
        (entry) => entry.kind === "tracked" || entry.kind === "untracked",
      ) ?? [],
    [checkout?.status.entries],
  );
  const includedPaths = useMemo(() => new Set(included), [included]);
  if (checkout === null) {
    return null;
  }
  const disabled = state.busy || !state.fresh || state.pendingOperation !== null;
  function create() {
    state
      .managed({
        kind: "worktree_create",
        ...(projectId === undefined ? {} : { project_id: projectId }),
        request: { managed_root: root, destination, commit, include_paths: included },
      })
      .then((result) => {
        if (result?.kind !== "worktree") {
          return;
        }
        const created = result.worktree;
        if (created.preparation_error !== null) {
          setMessage(
            `Checkout created at ${created.target.root ?? destination}; preparation needs attention: ${created.preparation_error}`,
          );
          return;
        }
        if (result.receipt.state === "completed" && created.target.root !== null) {
          setMessage(`Checkout ready at ${created.target.root}`);
          onWorktreeCreated?.(created.target.root);
        }
      })
      .catch(state.fail);
  }
  return (
    <div className="grid gap-6">
      <form
        className="grid max-w-xl gap-3"
        onSubmit={(event) => {
          event.preventDefault();
          create();
        }}
      >
        <h3 className="text-sm font-medium">New worktree</h3>
        <p className="text-xs leading-5 text-muted">
          Start from a commit with detached HEAD. Your source checkout stays in place. Create a
          branch later when you need one.
        </p>
        <TextField>
          <Label>Starting commit</Label>
          <Input
            value={commit}
            onChange={(event) => {
              updateDraft({ startingCommit: event.target.value });
            }}
            required
          />
        </TextField>
        <TextField>
          <Label>Managed worktree directory on this node</Label>
          <Input
            value={root}
            onChange={(event) => {
              updateDraft({ managedRoot: event.target.value });
            }}
            required
          />
        </TextField>
        <TextField>
          <Label>New checkout path</Label>
          <Input
            value={destination}
            onChange={(event) => {
              updateDraft({ worktreeDestination: event.target.value });
            }}
            required
          />
        </TextField>
        <fieldset className="grid gap-2">
          <legend className="mb-2 text-xs font-medium">Include selected local changes</legend>
          <p className="text-xs leading-5 text-muted">
            Each selected file preserves its staged and unstaged versions. Including changes
            requires the source HEAD as the starting commit. Ignored files are excluded.
          </p>
          <VirtualList
            items={candidates}
            getItemKey={candidateKey}
            label="Local changes to include"
            estimateSize={28}
            className="h-32"
            renderItem={(entry) => (
              <label className="flex items-center gap-2 py-1 text-xs">
                <input
                  type="checkbox"
                  checked={includedPaths.has(entry.path)}
                  onChange={(event) => {
                    const paths =
                      entry.original_path === null
                        ? [entry.path]
                        : [entry.path, entry.original_path];
                    updateDraft({
                      includedPaths: event.target.checked
                        ? [...new Set([...included, ...paths])]
                        : included.filter((path) => !paths.includes(path)),
                    });
                  }}
                />
                {entry.path}
                {entry.kind === "untracked" ? " · untracked" : ""}
              </label>
            )}
          />
        </fieldset>
        <Button
          type="submit"
          size="sm"
          isDisabled={disabled || !root.trim() || !destination.trim() || !commit.trim()}
        >
          Create detached worktree
        </Button>
        {message === null ? null : <output className="text-xs leading-5">{message}</output>}
      </form>
      <Archives
        state={state}
        root={root}
        destination={destination}
        onRestored={onWorktreeCreated}
      />
    </div>
  );
}

function Archives({
  state,
  root,
  destination,
  onRestored,
}: {
  state: State;
  root: string;
  destination: string;
  onRestored: ((path: string) => void) | undefined;
}) {
  const [archives, setArchives] = useState<ArchiveReceipt[]>([]);
  const [unavailable, setUnavailable] = useState<UnavailableEntry[]>([]);
  const [message, setMessage] = useState<string | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  const { remote, checkout } = state;
  useEffect(() => {
    let active = true;
    if (remote === null || checkout === null || !state.connected) {
      return () => {
        active = false;
      };
    }
    async function list() {
      if (remote === null || checkout === null) {
        return;
      }
      try {
        const result = await readArchives(remote, checkout.target);
        if (active) {
          setArchives(result.archives);
          setUnavailable(result.unavailable);
          setFailure(null);
        }
      } catch (error: unknown) {
        if (active) {
          setFailure(error instanceof Error ? error.message : "Unable to list archives.");
        }
      }
    }
    list().catch(console.error);
    return () => {
      active = false;
    };
  }, [remote, checkout, state.connected]);
  const disabled = state.busy || !state.fresh || state.pendingOperation !== null;
  function snapshot() {
    state
      .managed({ kind: "archive_create", archive_id: crypto.randomUUID() })
      .then((result) => {
        if (result?.kind === "archive") {
          setMessage(
            `Snapshot ${result.archive.archive_id} saved. ${result.archive.cleanup_blocked_reason}`,
          );
        }
      })
      .catch(state.fail);
  }
  function restore(archiveId: string) {
    state
      .managed({ kind: "archive_restore", archive_id: archiveId, managed_root: root, destination })
      .then((result) => {
        if (result?.kind === "archive") {
          setMessage(`Snapshot restored to ${destination}. The source checkout was retained.`);
          const restored = result.archive.restored_target;
          if (
            result.receipt.state === "completed" &&
            restored?.root !== undefined &&
            restored.root !== null
          ) {
            onRestored?.(restored.root);
          }
        }
      })
      .catch(state.fail);
  }
  function check(archiveId: string) {
    if (remote === null || checkout === null) {
      return;
    }
    remote
      .request({ kind: "cleanup_check", target: checkout.target, archive_id: archiveId })
      .then((result) => {
        if (result.kind === "cleanup") {
          setMessage(result.eligibility.reason);
        }
      })
      .catch(state.fail);
  }
  return (
    <div className="grid gap-3 border-t border-border pt-5">
      <h3 className="text-sm font-medium">Archive and recovery</h3>
      <p className="text-xs leading-5 text-muted">
        A snapshot includes tracked, untracked and ignored files, stored privately on this node. It
        preserves checkout data separately from conversation archiving. Physical cleanup remains
        blocked until the host can prove that writers have stopped and restoration is verified.
      </p>
      <div>
        <Button size="sm" isDisabled={disabled} onPress={snapshot}>
          Create recovery snapshot
        </Button>
      </div>
      {failure === null ? null : (
        <p role="alert" className="text-xs text-danger">
          {failure}
        </p>
      )}
      {message === null ? null : <output className="text-xs leading-5">{message}</output>}
      {unavailable.length === 0 ? null : (
        <div role="alert" className="text-xs text-danger">
          <p>Some recovery snapshots could not be read. They have not been removed.</p>
          <VirtualList
            items={unavailable}
            getItemKey={unavailableKey}
            label="Unreadable recovery snapshots"
            estimateSize={40}
            className="h-32"
            renderItem={(entry) => <p className="py-1 break-words">{entry.message}</p>}
          />
        </div>
      )}
      {archives.length === 0 ? null : (
        <VirtualList
          items={archives}
          getItemKey={archiveKey}
          label="Recovery snapshots"
          estimateSize={112}
          className="h-64"
          renderItem={(archive) => (
            <div className="grid gap-2 py-2 text-xs">
              <span className="font-mono break-all">{archive.archive_id}</span>
              <p className="text-muted">
                {archive.files} files · {archive.head.slice(0, 8)} ·{" "}
                {archive.restore_verified ? "Restore verified" : "Restore verification pending"}
              </p>
              <div className="flex flex-wrap gap-2">
                <Button
                  size="sm"
                  isDisabled={disabled || !root.trim() || !destination.trim()}
                  onPress={() => {
                    restore(archive.archive_id);
                  }}
                >
                  Restore to new checkout path
                </Button>
                <Button
                  size="sm"
                  variant="tertiary"
                  isDisabled={disabled}
                  onPress={() => {
                    check(archive.archive_id);
                  }}
                >
                  Check cleanup eligibility
                </Button>
              </div>
            </div>
          )}
        />
      )}
    </div>
  );
}
