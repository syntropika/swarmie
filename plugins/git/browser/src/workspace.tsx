import { Button, VirtualList } from "@swarmie/ui-web";
import { CodePreview } from "@swarmie/ui-web/code";
import { FileCode2, X } from "lucide-react";
import { useEffect, useId, useMemo, useState } from "react";

import { ChangedFiles } from "./changed-files";
import { errorMessage } from "./checkout-data";
import type { Checkout } from "./checkout-data";
import { CompactFiles } from "./compact-files";
import { diffLines } from "./diff";
import { DiffView } from "./diff-view";
import { compactSelection } from "./file-selection";
import type { Selection } from "./file-selection";
import type { DiffMode, GitOperation } from "./protocol";
import type { GitRemote } from "./remote";
import type { ChangesFilter } from "./view-controls";

export type { Selection } from "./file-selection";
function selectionKey(selection: Selection) {
  return JSON.stringify([selection.path, selection.source, selection.staged]);
}

function tabLabel(file: Selection) {
  const name = file.path.split(/[\\/]/u).at(-1) ?? file.path;
  if (file.source === "diff") {
    return `${name} · ${file.staged ? "Staged" : "Changes"}`;
  }
  return name;
}

function OpenFiles({
  files,
  selection,
  select,
  close,
  id,
}: {
  id: string;
  files: Selection[];
  selection: Selection | null;
  select: (selection: Selection) => void;
  close: (selection: Selection) => void;
}) {
  if (files.length === 0) {
    return null;
  }
  return (
    <div
      role="tablist"
      aria-label="Open files"
      className="bg-surface-secondary flex h-10 shrink-0 overflow-x-auto border-b border-border"
    >
      {files.map((file, index) => {
        const selected = selection !== null && selectionKey(selection) === selectionKey(file);
        return (
          <div
            key={selectionKey(file)}
            role="presentation"
            className={`flex h-full max-w-64 shrink-0 items-center border-r border-b-2 border-border ${selected ? "border-b-brand-accent bg-surface" : "border-b-transparent"}`}
          >
            <button
              type="button"
              role="tab"
              id={`${id}-${index}`}
              aria-selected={selected}
              aria-controls={`${id}-panel`}
              tabIndex={selected ? 0 : -1}
              className={`flex h-full min-w-0 items-center gap-2 px-3 text-xs ${selected ? "text-foreground" : "text-muted hover:text-foreground"}`}
              onClick={() => {
                select(file);
              }}
              onKeyDown={(event) => {
                if (event.key === "Delete") {
                  event.preventDefault();
                  close(file);
                  return;
                }
                let offset = 0;
                if (event.key === "ArrowRight") {
                  offset = 1;
                }
                if (event.key === "ArrowLeft") {
                  offset = -1;
                }
                let next = (index + offset + files.length) % files.length;
                if (event.key === "Home") {
                  next = 0;
                }
                if (event.key === "End") {
                  next = files.length - 1;
                }
                if (!offset && event.key !== "Home" && event.key !== "End") {
                  return;
                }
                event.preventDefault();
                const nextFile = files[next];
                if (nextFile) {
                  select(nextFile);
                }
                const tabs = event.currentTarget
                  .closest('[role="tablist"]')
                  ?.querySelectorAll<HTMLButtonElement>('[role="tab"]');
                tabs?.[next]?.focus();
              }}
              title={file.path}
            >
              <FileCode2 size={13} className="shrink-0 text-muted" />
              <span className="truncate">{tabLabel(file)}</span>
            </button>
            <button
              type="button"
              tabIndex={selected ? 0 : -1}
              className="mr-2 grid size-5 shrink-0 place-items-center rounded text-muted hover:bg-default hover:text-foreground focus-visible:outline-2"
              aria-label={`Close ${file.path}`}
              onClick={() => {
                close(file);
              }}
            >
              <X size={12} />
            </button>
          </div>
        );
      })}
    </div>
  );
}

type DocumentResult = { key: string; text: string; truncated: boolean; error: string | null };

function useDocument(
  remote: GitRemote,
  checkout: Checkout,
  selection: Selection | null,
  layout: DiffMode,
) {
  const [result, setResult] = useState<DocumentResult | null>(null);
  const path = selection?.path ?? null;
  const source = selection?.source ?? "file";
  const staged = selection?.staged ?? false;
  const key = JSON.stringify([path, source, staged, layout, checkout.target.fingerprint]);
  useEffect(() => {
    let active = true;
    if (path === null) {
      return () => {
        active = false;
      };
    }
    const request =
      source === "diff"
        ? remote.request({
            kind: "diff",
            target: checkout.target,
            path,
            staged,
            mode: layout,
          })
        : remote.request({
            kind: "file",
            target: checkout.target,
            path,
            revision: null,
          });
    async function read() {
      try {
        const response = await request;
        if (!active) {
          return;
        }
        if (response.kind === "diff") {
          setResult({ key, ...response.diff, error: null });
        } else if (response.kind === "file") {
          setResult({ key, text: response.text, truncated: response.truncated, error: null });
        }
      } catch (error: unknown) {
        if (active) {
          setResult({ key, text: "", truncated: false, error: errorMessage(error) });
        }
      }
    }
    read().catch(console.error);
    return () => {
      active = false;
    };
  }, [remote, checkout.target, path, source, staged, layout, key]);
  return result?.key === key ? result : null;
}

export function Empty({ title, detail }: { title: string; detail: string }) {
  return (
    <div className="m-auto max-w-md px-6 py-10 text-center">
      <h2 className="text-sm font-medium">{title}</h2>
      <p className="mt-2 text-sm leading-6 text-muted">{detail}</p>
    </div>
  );
}

function DocumentContent({
  result,
  selection,
  layout,
}: {
  result: DocumentResult | null;
  selection: Selection | null;
  layout: DiffMode;
}) {
  if (selection === null) {
    return (
      <Empty
        title="Review your work"
        detail="Open a file to read it, or choose a change to compare it with the captured checkout."
      />
    );
  }
  if (result === null) {
    return <output className="p-6 text-sm text-muted">Loading file…</output>;
  }
  if (result.error !== null) {
    return (
      <p role="alert" className="p-6 text-sm text-danger">
        {result.error}
      </p>
    );
  }
  return (
    <>
      {selection.source === "diff" ? (
        <DiffView
          text={result.text}
          mode={layout}
          path={selection.path}
          staged={selection.staged}
        />
      ) : (
        <CodePreview code={result.text} path={selection.path} />
      )}
      {result.truncated ? (
        <output className="border-t border-border px-4 py-2 text-xs text-muted">
          Preview truncated. Review the complete file before committing.
        </output>
      ) : null}
    </>
  );
}

function fileKey(path: string) {
  return path;
}

function ExploreFiles({
  checkout,
  selection,
  select,
  compact,
}: {
  checkout: Checkout;
  selection: Selection | null;
  select: (selection: Selection) => void;
  compact: boolean;
}) {
  if (checkout.files.state === "unavailable") {
    return (
      <p role="alert" className="p-4 text-xs text-danger">
        Files unavailable: {checkout.files.error}
      </p>
    );
  }
  if (checkout.files.value.length === 0) {
    return <p className="p-4 text-xs text-muted">No tracked or untracked files.</p>;
  }
  return (
    <VirtualList
      items={checkout.files.value}
      getItemKey={fileKey}
      label="Repository files"
      estimateSize={compact ? 44 : 32}
      className="h-full"
      renderItem={(file) => (
        <button
          type="button"
          onClick={() => {
            select({ path: file, staged: false, source: "file" });
          }}
          className={`flex w-full items-center gap-2 px-4 py-2 text-left text-xs hover:bg-default focus-visible:outline-2 focus-visible:-outline-offset-2 ${selection?.path === file ? "bg-default" : ""}`}
        >
          <FileCode2 size={14} className="shrink-0 text-muted" />
          <span className="truncate" title={file}>
            {file}
          </span>
        </button>
      )}
    />
  );
}

function selectedPaths(checkout: Checkout, selection: Selection) {
  const original = checkout.status.entries.find(
    (entry) => entry.path === selection.path,
  )?.original_path;
  if (typeof original === "string" && original.length > 0) {
    return [original, selection.path];
  }
  return [selection.path];
}

function changeCounts(result: DocumentResult | null, selection: Selection | null) {
  const lines = selection?.source === "diff" && result ? diffLines(result.text) : [];
  return {
    added: lines.filter((line) => line.kind === "added").length,
    removed: lines.filter((line) => line.kind === "removed").length,
  };
}

function ChangeCounts({
  visible,
  added,
  removed,
}: {
  visible: boolean;
  added: number;
  removed: number;
}) {
  return visible ? (
    <span
      className="flex shrink-0 items-center gap-2 font-mono text-xs"
      aria-label={`${added} added lines, ${removed} removed lines`}
    >
      <span data-git-status="added">+{added}</span>
      <span data-git-status="removed">−{removed}</span>
    </span>
  ) : null;
}

function StageFile({
  selection,
  checkout,
  disabled,
  mutate,
  onError,
}: {
  selection: Selection | null;
  checkout: Checkout;
  disabled: boolean;
  mutate: (operation: GitOperation) => Promise<boolean>;
  onError: (error: unknown) => void;
}) {
  return selection === null ? null : (
    <Button
      size="sm"
      variant="secondary"
      isDisabled={
        disabled ||
        !checkout.status.entries.some(
          (entry) => entry.path === selection.path && entry.kind !== "unmerged",
        )
      }
      onPress={() => {
        mutate({
          kind: selection.staged ? "unstage" : "stage",
          paths: selectedPaths(checkout, selection),
        }).catch(onError);
      }}
    >
      {selection.staged ? "Unstage file" : "Stage file"}
    </Button>
  );
}

function documentAccessibility(
  id: string,
  files: Selection[],
  selection: Selection | null,
  compact: boolean,
) {
  if (compact || selection === null) {
    return {};
  }
  const index = files.findIndex((file) => selectionKey(file) === selectionKey(selection));
  if (index === -1) {
    return {};
  }
  return { role: "tabpanel" as const, id: `${id}-panel`, "aria-labelledby": `${id}-${index}` };
}

export function GitWorkspace({
  checkout,
  remote,
  mode,
  disabled,
  mutate,
  onError,
  layout,
  filter,
  compact,
  onLayoutChange,
  onFilterChange,
}: {
  compact: boolean;
  onLayoutChange: (layout: DiffMode) => void;
  onFilterChange: (filter: ChangesFilter) => void;
  checkout: Checkout;
  remote: GitRemote;
  mode: "explore" | "changes";
  disabled: boolean;
  mutate: (operation: GitOperation) => Promise<boolean>;
  onError: (error: unknown) => void;
  layout: DiffMode;
  filter: "all" | "staged" | "unstaged";
}) {
  const id = useId();
  const [chosenSelection, setChosenSelection] = useState<Selection | null>(() =>
    compact && mode === "changes" ? compactSelection(checkout.status.entries, filter, null) : null,
  );
  const [pickerOpen, setPickerOpen] = useState(false);
  const selection = useMemo(
    () =>
      compact && mode === "changes"
        ? compactSelection(checkout.status.entries, filter, chosenSelection)
        : chosenSelection,
    [compact, mode, checkout.status.entries, filter, chosenSelection],
  );
  const [files, setFiles] = useState<Selection[]>(() =>
    chosenSelection === null ? [] : [chosenSelection],
  );
  function open(file: Selection) {
    setFiles((current) =>
      current.some((entry) => selectionKey(entry) === selectionKey(file))
        ? current
        : [...current, file],
    );
    setChosenSelection(file);
    setPickerOpen(false);
  }
  function close(file: Selection) {
    const remaining = files.filter((entry) => selectionKey(entry) !== selectionKey(file));
    setFiles(remaining);
    if (selection !== null && selectionKey(file) === selectionKey(selection)) {
      setChosenSelection(remaining.at(-1) ?? null);
    }
  }
  const result = useDocument(remote, checkout, selection, layout);
  const staged = selection?.staged === true ? " · staged" : "";
  const { added, removed } = useMemo(() => changeCounts(result, selection), [result, selection]);
  const fileList =
    mode === "explore" ? (
      <ExploreFiles checkout={checkout} selection={selection} select={open} compact={compact} />
    ) : (
      <ChangedFiles
        checkout={checkout}
        selection={selection}
        disabled={disabled}
        select={open}
        mutate={mutate}
        onError={onError}
        filter={filter}
        compact={compact}
      />
    );
  const counts = (
    <ChangeCounts
      visible={selection?.source === "diff" && result !== null}
      added={added}
      removed={removed}
    />
  );
  const stageButton = (
    <StageFile
      selection={selection}
      checkout={checkout}
      disabled={disabled}
      mutate={mutate}
      onError={onError}
    />
  );
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      {compact ? (
        <CompactFiles
          pickerOpen={pickerOpen}
          setPickerOpen={setPickerOpen}
          selection={selection}
          mode={mode}
          filter={filter}
          layout={layout}
          onLayoutChange={onLayoutChange}
          onFilterChange={onFilterChange}
          counts={counts}
          stageButton={stageButton}
          fileList={fileList}
        />
      ) : null}
      <div className="flex min-h-0 flex-1">
        {compact ? null : (
          <aside
            className="w-64 shrink-0 resize-x overflow-hidden border-r border-border"
            aria-label={mode === "explore" ? "Repository files" : "Changed files"}
          >
            {fileList}
          </aside>
        )}
        <div className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden">
          {compact ? null : (
            <>
              <OpenFiles
                id={id}
                files={files}
                selection={selection}
                select={setChosenSelection}
                close={close}
              />
              <div className="flex flex-wrap items-center gap-2 border-b border-border px-4 py-2">
                <span className="min-w-0 flex-1 truncate text-xs" title={selection?.path}>
                  {selection?.path ?? "Select a file"}
                  {staged}
                </span>
                {counts}
                {selection?.source === "diff" ? (
                  <Button
                    size="sm"
                    variant="tertiary"
                    onPress={() => {
                      open({ path: selection.path, staged: false, source: "file" });
                    }}
                  >
                    Open file
                  </Button>
                ) : null}
                {stageButton}
              </div>
            </>
          )}
          <div
            {...documentAccessibility(id, files, selection, compact)}
            className="flex min-h-0 flex-1 flex-col"
          >
            <DocumentContent result={result} selection={selection} layout={layout} />
          </div>
        </div>
      </div>
    </div>
  );
}
