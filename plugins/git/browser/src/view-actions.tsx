import { ActionMenuPopover, Button, Dropdown, Label } from "@swarmie/ui-web";
import { GitBranch, GitCommitHorizontal, MoreHorizontal } from "lucide-react";

import type { CheckoutState, Panel } from "./view-panels";

export function branchLabel(state: CheckoutState, path: string) {
  const target = state.checkout?.target;
  if (target?.branch !== undefined && target.branch !== null) {
    return target.branch;
  }
  if (target?.head !== undefined && target.head !== null) {
    return `Detached · ${target.head.slice(0, 8)}`;
  }
  return path;
}

export function GitMenu({
  panel,
  setPanel,
  state,
  onStartConversation,
  compact = false,
  mode,
  onToggleMode,
}: {
  panel: Panel;
  setPanel: (panel: Panel) => void;
  state: CheckoutState;
  onStartConversation: (() => void) | undefined;
  compact?: boolean;
  mode?: "explore" | "changes";
  onToggleMode?: () => void;
}) {
  const disabled = state.busy || !state.fresh || state.pendingOperation !== null;
  return (
    <Dropdown>
      <Button size="sm" variant="tertiary" aria-label="More Git actions">
        <MoreHorizontal size={16} />
      </Button>
      <ActionMenuPopover>
        <Dropdown.Menu
          aria-label="Git actions"
          onAction={(key) => {
            if (key === "refresh") {
              state.refresh().catch(state.fail);
            }
            if (key === "mode") {
              onToggleMode?.();
            }
            if (key === "branch") {
              setPanel("branch");
            }
            if (key === "sync" || key === "history" || key === "worktrees") {
              setPanel(panel === key ? null : key);
            }
            if (key === "conversation") {
              onStartConversation?.();
            }
          }}
        >
          {compact ? (
            <>
              <Dropdown.Item
                id="mode"
                textValue={mode === "changes" ? "Explore files" : "Review changes"}
              >
                <Label>{mode === "changes" ? "Explore files" : "Review changes"}</Label>
              </Dropdown.Item>
              <Dropdown.Item id="refresh" textValue="Refresh checkout" isDisabled={state.busy}>
                <Label>Refresh checkout</Label>
              </Dropdown.Item>
              <Dropdown.Item
                id="branch"
                textValue="Create branch"
                isDisabled={disabled || state.checkout === null}
              >
                <Label>Create branch</Label>
              </Dropdown.Item>
            </>
          ) : null}
          <Dropdown.Item
            id="sync"
            textValue="Fetch / Push"
            isDisabled={disabled || state.checkout === null}
          >
            <Label>Fetch / Push</Label>
          </Dropdown.Item>
          <Dropdown.Item id="history" textValue="History" isDisabled={state.checkout === null}>
            <Label>History</Label>
          </Dropdown.Item>
          <Dropdown.Item
            id="worktrees"
            textValue="Worktrees"
            isDisabled={disabled || state.checkout === null}
          >
            <Label>Worktrees</Label>
          </Dropdown.Item>
          {onStartConversation ? (
            <Dropdown.Item
              id="conversation"
              textValue="Start conversation here"
              isDisabled={disabled || state.checkout === null}
            >
              <Label>Start conversation here</Label>
            </Dropdown.Item>
          ) : null}
        </Dropdown.Menu>
      </ActionMenuPopover>
    </Dropdown>
  );
}

export function GitActions({
  panel,
  setPanel,
  state,
  path,
  nodeLabel,
  onStartConversation,
  compact,
}: {
  compact: boolean;
  path: string;
  nodeLabel: string;
  onStartConversation: (() => void) | undefined;
  panel: Panel;
  setPanel: (panel: Panel) => void;
  state: CheckoutState;
}) {
  const disabled = state.busy || !state.fresh || state.pendingOperation !== null;
  const staged =
    state.checkout?.status.entries.filter(
      (entry) => entry.kind === "tracked" && entry.index !== "." && entry.index !== " ",
    ).length ?? 0;
  if (compact) {
    return (
      <footer
        data-git-commit-bar=""
        className="flex shrink-0 items-center justify-between gap-3 border-t border-border px-4 py-3"
        aria-label="Checkout status and actions"
      >
        <span className="text-xs text-muted">
          {state.checkout === null
            ? "Changes unavailable"
            : `${staged} ${staged === 1 ? "file" : "files"} staged`}
        </span>
        <Button
          size="sm"
          variant="primary"
          isDisabled={disabled || state.checkout === null || state.checkout.target.bare}
          onPress={() => {
            setPanel("commit");
          }}
        >
          <GitCommitHorizontal size={16} />
          Commit…
        </Button>
      </footer>
    );
  }
  return (
    <footer
      className="flex flex-wrap items-center gap-x-3 gap-y-1 border-t border-border px-4 py-2 text-xs"
      aria-label="Checkout status and actions"
    >
      <span className="flex min-w-0 items-center gap-2" title={path}>
        <GitBranch size={14} className="shrink-0 text-muted" />
        <span className="max-w-40 truncate">
          {path.split(/[\\/]/u).findLast((part) => part.length > 0)}
        </span>
        <span className="text-muted">·</span>
        {nodeLabel}
      </span>
      <span className="border-l border-border pl-3 text-muted">{branchLabel(state, path)}</span>
      <div className="flex-1" />
      <Button
        size="sm"
        variant={panel === "branch" ? "secondary" : "tertiary"}
        isDisabled={disabled || state.checkout === null}
        onPress={() => {
          setPanel(panel === "branch" ? null : "branch");
        }}
      >
        <GitBranch size={14} />
        Create branch
      </Button>
      <GitMenu
        panel={panel}
        setPanel={setPanel}
        state={state}
        onStartConversation={onStartConversation}
      />
    </footer>
  );
}
