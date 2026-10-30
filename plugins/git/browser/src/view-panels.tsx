import { Button, Drawer } from "@swarmie/ui-web";
import { X } from "lucide-react";

import { BranchPanel, CommitPanel, HistoryPanel, SyncPanel } from "./actions";
import type { DraftProps } from "./drafts";
import { RecoveryNotice } from "./recovery-notice";
import type { useCheckout } from "./use-checkout";
import { WorktreesPanel } from "./worktrees-panel";

export type Panel = "commit" | "branch" | "sync" | "history" | "worktrees" | null;
export type CheckoutState = ReturnType<typeof useCheckout>;

export function GitMessages({ state }: { state: CheckoutState }) {
  return (
    <>
      {state.error === null ? null : (
        <p role="alert" className="border-b border-border px-4 py-3 text-sm text-danger">
          {state.error}
        </p>
      )}
      {state.receipt === null ? null : (
        <output className="border-b border-border px-4 py-3 text-sm">
          {state.receipt.message}
        </output>
      )}
      {state.operationWarnings.length === 0 ? null : (
        <div role="alert" className="border-b border-border px-4 py-3 text-sm text-danger">
          <p>
            Recovery records are incomplete. New changes are blocked until the node can read them.
          </p>
          <ul>
            {[...new Set(state.operationWarnings)].map((message) => (
              <li key={message}>{message}</li>
            ))}
          </ul>
        </div>
      )}
      <RecoveryNotice key={state.pendingOperation} state={state} />
    </>
  );
}

function ActionContent({
  state,
  panel,
  close,
  projectId,
  onWorktreeCreated,
  drafts,
  updateDraft,
}: DraftProps & {
  state: CheckoutState;
  panel: Exclude<Panel, null>;
  close: () => void;
  projectId: string | undefined;
  onWorktreeCreated: ((path: string) => void) | undefined;
}) {
  if (state.checkout === null) {
    return null;
  }
  const props = {
    checkout: state.checkout,
    disabled: state.busy || !state.fresh || state.pendingOperation !== null,
    mutate: state.mutate,
    close,
    branchPrefix: state.settings?.branch_prefix ?? "",
    commitGuidance: state.settings?.commit_guidance ?? "",
    runCommitHooks: state.settings?.run_commit_hooks ?? false,
    drafts,
    updateDraft,
  };
  if (panel === "history") {
    return <HistoryPanel checkout={state.checkout} />;
  }
  if (panel === "branch") {
    return <BranchPanel {...props} />;
  }
  if (panel === "sync") {
    return <SyncPanel {...props} />;
  }
  if (panel === "worktrees") {
    return (
      <WorktreesPanel
        state={state}
        projectId={projectId}
        onWorktreeCreated={onWorktreeCreated}
        drafts={drafts}
        updateDraft={updateDraft}
      />
    );
  }
  return <CommitPanel {...props} />;
}

const titles = {
  commit: "Review commit",
  branch: "Create branch",
  sync: "Fetch / Push",
  history: "History",
  worktrees: "Worktrees",
};
export function CheckoutPanel({
  state,
  panel,
  setPanel,
  compact,
  projectId,
  onWorktreeCreated,
  drafts,
  updateDraft,
}: DraftProps & {
  state: CheckoutState;
  panel: Panel;
  setPanel: (panel: Panel) => void;
  compact: boolean;
  projectId: string | undefined;
  onWorktreeCreated: ((path: string) => void) | undefined;
}) {
  if (panel === null || state.checkout === null) {
    return null;
  }
  const content = (
    <ActionContent
      key={panel}
      drafts={drafts}
      updateDraft={updateDraft}
      state={state}
      panel={panel}
      close={() => {
        setPanel(null);
      }}
      projectId={projectId}
      onWorktreeCreated={onWorktreeCreated}
    />
  );
  if (!compact) {
    return (
      <div className="max-h-96 shrink-0 overflow-auto border-t border-border px-4 py-4">
        {content}
      </div>
    );
  }
  return (
    <Drawer
      isOpen
      onOpenChange={(open) => {
        if (!open) {
          setPanel(null);
        }
      }}
    >
      <Drawer.Backdrop>
        <Drawer.Content placement="bottom" data-git-workspace="" data-git-compact="true">
          <Drawer.Dialog className="max-h-[85dvh]">
            <Drawer.Handle />
            <Drawer.Header className="flex flex-row items-center justify-between">
              <Drawer.Heading>{titles[panel]}</Drawer.Heading>
              <Button slot="close" variant="tertiary" aria-label="Close Git action">
                <X size={18} />
              </Button>
            </Drawer.Header>
            <Drawer.Body className="overflow-auto">
              <GitMessages state={state} />
              {content}
            </Drawer.Body>
          </Drawer.Dialog>
        </Drawer.Content>
      </Drawer.Backdrop>
    </Drawer>
  );
}
