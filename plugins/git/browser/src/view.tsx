import { Button, ToggleButton, ToggleButtonGroup } from "@swarmie/ui-web";
import { FolderOpen, GitCommitHorizontal, RefreshCw } from "lucide-react";
import { useState } from "react";

import type { DiffMode } from "./protocol";
import type { GitConnection } from "./remote";
import { useCheckout } from "./use-checkout";
import { useCompactView } from "./use-compact-view";
import { GitActions, GitMenu, branchLabel } from "./view-actions";
import { DiffLayout } from "./view-controls";
import type { ChangesFilter } from "./view-controls";
import { CheckoutPanel, GitMessages } from "./view-panels";
import type { Panel, CheckoutState } from "./view-panels";
import { Empty, GitWorkspace } from "./workspace";

export type GitViewProps = {
  connection: GitConnection | null;
  path: string;
  projectId?: string;
  onWorktreeCreated?: (path: string) => void;
  nodeLabel?: string;
  onStartConversation?: () => void;
};
function CheckoutContent({
  state,
  mode,
  panel,
  setPanel,
  projectId,
  onWorktreeCreated,
  layout,
  filter,
  compact,
  onLayoutChange,
  onFilterChange,
}: {
  compact: boolean;
  onLayoutChange: (layout: DiffMode) => void;
  onFilterChange: (filter: ChangesFilter) => void;
  layout: DiffMode;
  filter: "all" | "staged" | "unstaged";
  state: CheckoutState;
  mode: "explore" | "changes";
  panel: Panel;
  setPanel: (panel: Panel) => void;
  projectId: string | undefined;
  onWorktreeCreated: ((path: string) => void) | undefined;
}) {
  if (!state.loaded) {
    return <output className="p-6 text-sm text-muted">Opening checkout…</output>;
  }
  if (state.checkout === null || state.remote === null) {
    if (state.settings !== null && state.error !== null) {
      return (
        <Empty
          title="Changes unavailable"
          detail="Git settings remain readable. Refresh the repository to capture a complete observation before reviewing changes or making writes."
        />
      );
    }
    return (
      <Empty
        title={state.error === null ? "This folder has no Git repository" : "Git is unavailable"}
        detail={
          state.error === null
            ? "The project can still be used without Git. Select a repository folder to review changes."
            : "Check the node connection and its native Git capability, then refresh."
        }
      />
    );
  }
  return (
    <>
      <GitWorkspace
        checkout={state.checkout}
        remote={state.remote}
        mode={mode}
        disabled={state.busy || !state.fresh || state.pendingOperation !== null}
        mutate={state.mutate}
        onError={state.fail}
        layout={layout}
        filter={filter}
        compact={compact}
        onLayoutChange={onLayoutChange}
        onFilterChange={onFilterChange}
      />
      <CheckoutPanel
        state={state}
        panel={panel}
        setPanel={setPanel}
        compact={compact}
        projectId={projectId}
        onWorktreeCreated={onWorktreeCreated}
      />
    </>
  );
}

export function GitView({
  connection,
  path,
  projectId,
  onWorktreeCreated,
  nodeLabel,
  onStartConversation,
}: GitViewProps) {
  const state = useCheckout(connection, path, projectId);
  const { ref, compact, short } = useCompactView();
  const [mode, setMode] = useState<"explore" | "changes">("changes");
  const [panel, setPanel] = useState<Panel>(null);
  const [chosenLayout, setChosenLayout] = useState<DiffMode | null>(null);
  const [filter, setFilter] = useState<"all" | "staged" | "unstaged">("all");
  const defaultLayout = compact ? "unified" : state.settings?.diff_layout;
  const layout = chosenLayout ?? defaultLayout ?? "unified";
  const disabled =
    state.busy || !state.fresh || state.pendingOperation !== null || state.checkout === null;
  const resolvedNodeLabel = nodeLabel ?? connection?.nodeId ?? "Not connected";
  return (
    <section
      ref={ref}
      data-git-compact={compact}
      data-git-short={short}
      data-git-workspace=""
      className="flex h-full min-h-0 flex-col bg-surface text-foreground"
      aria-label="Project files and Git changes"
    >
      {compact ? (
        <header className="flex shrink-0 items-center justify-between gap-2 border-b border-border px-3 py-2">
          <div className="min-w-0">
            <h2 className="text-sm font-medium">{mode === "changes" ? "Changes" : "Explore"}</h2>
            <p className="truncate text-xs text-muted" title={path}>
              {path.split(/[\\/]/u).findLast((part) => part.length > 0)} · {resolvedNodeLabel} ·{" "}
              {branchLabel(state, path)}
            </p>
          </div>
          <GitMenu
            panel={panel}
            setPanel={setPanel}
            state={state}
            compact
            mode={mode}
            onToggleMode={() => {
              setMode(mode === "changes" ? "explore" : "changes");
              setPanel(null);
            }}
            onStartConversation={onStartConversation}
          />
        </header>
      ) : (
        <div className="flex shrink-0 flex-wrap items-center gap-3 border-b border-border px-4 py-3">
          <ToggleButtonGroup
            data-git-segments=""
            aria-label="File view mode"
            selectionMode="single"
            disallowEmptySelection
            selectedKeys={[mode]}
            size="sm"
            onSelectionChange={(keys) => {
              const [next] = keys;
              if (next === "explore" || next === "changes") {
                setMode(next);
                setPanel(null);
              }
            }}
          >
            <ToggleButton id="explore">
              <FolderOpen size={14} />
              Explore
            </ToggleButton>
            <ToggleButton id="changes">Changes</ToggleButton>
          </ToggleButtonGroup>
          <div className="flex-1" />
          {mode === "changes" ? (
            <>
              <select
                aria-label="Changes to review"
                value={filter}
                onChange={(event) => {
                  const { value } = event.target;
                  if (value === "all" || value === "staged" || value === "unstaged") {
                    setFilter(value);
                  }
                }}
                className="h-9 rounded-lg border border-border bg-surface px-3 text-xs"
              >
                <option value="all">All changes</option>
                <option value="staged">Staged</option>
                <option value="unstaged">Unstaged</option>
              </select>
              <DiffLayout value={layout} onChange={setChosenLayout} />
              <Button
                size="sm"
                variant="primary"
                isDisabled={disabled || state.checkout?.target.bare === true}
                onPress={() => {
                  setPanel(panel === "commit" ? null : "commit");
                }}
              >
                <GitCommitHorizontal size={15} />
                Commit
              </Button>
            </>
          ) : null}
          <Button
            size="sm"
            variant="tertiary"
            aria-label="Refresh checkout"
            isDisabled={state.busy}
            onPress={() => {
              state.refresh().catch(state.fail);
            }}
          >
            <RefreshCw size={15} />
          </Button>
        </div>
      )}
      <GitMessages state={state} />
      {connection === null ? (
        <Empty
          title="Connect an execution node"
          detail="Git runs on the node that owns this folder. Configure a connection to inspect files and changes."
        />
      ) : (
        <CheckoutContent
          state={state}
          mode={mode}
          panel={panel}
          setPanel={setPanel}
          projectId={projectId}
          onWorktreeCreated={onWorktreeCreated}
          layout={layout}
          filter={filter}
          compact={compact}
          onLayoutChange={setChosenLayout}
          onFilterChange={setFilter}
        />
      )}
      <GitActions
        compact={compact}
        panel={panel}
        setPanel={setPanel}
        state={state}
        path={path}
        nodeLabel={resolvedNodeLabel}
        onStartConversation={onStartConversation}
      />
    </section>
  );
}
