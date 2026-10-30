import { Button } from "@swarmie/ui-web";
import { useState } from "react";

import type { useCheckout } from "./use-checkout";

export function RecoveryNotice({ state }: { state: ReturnType<typeof useCheckout> }) {
  const [confirmed, setConfirmed] = useState(false);
  if (state.pendingOperation === null) {
    return null;
  }
  const unknown =
    state.receipt?.state === "unknown" && state.receipt.operation_id === state.pendingOperation;
  const inProgress =
    state.receipt?.state === "in_progress" && state.receipt.operation_id === state.pendingOperation;
  return (
    <div className="grid gap-3 border-b border-border px-4 py-3 text-sm">
      <p className="break-all">
        Operation {state.pendingOperation}{" "}
        {inProgress
          ? "is still running on this node. Wait for its outcome before another change."
          : "requires reconciliation before another change."}
      </p>
      <p className="text-xs leading-5 text-muted">
        Reconciliation checks an admitted operation. If this request never reached admission, it
        permanently blocks its identifier before allowing a new action.
      </p>
      <div>
        <Button
          size="sm"
          isDisabled={state.busy || !state.connected}
          onPress={() => {
            state.recover().catch(state.fail);
          }}
        >
          Reconcile outcome
        </Button>
      </div>
      {unknown ? (
        <div className="grid gap-3">
          <p className="text-xs leading-5 text-muted">
            The previous effect may already have happened. Its outcome remains unknown and this
            operation will never be replayed automatically.
          </p>
          <div>
            <Button
              size="sm"
              variant="secondary"
              isDisabled={state.busy || !state.connected}
              onPress={() => {
                setConfirmed(false);
                state.reviewUnknown().catch(state.fail);
              }}
            >
              Refresh checkout for review
            </Button>
          </div>
          {state.unknownReviewed ? (
            <>
              <label className="flex items-start gap-2 text-xs leading-5">
                <input
                  type="checkbox"
                  checked={confirmed}
                  onChange={(event) => {
                    setConfirmed(event.target.checked);
                  }}
                  className="mt-1"
                />
                I reviewed the refreshed checkout and remote state, and understand the previous
                effect may already have occurred.
              </label>
              <div>
                <Button
                  size="sm"
                  isDisabled={state.busy || !confirmed}
                  onPress={state.acknowledgeUnknown}
                >
                  Allow new operations
                </Button>
              </div>
              <p className="text-xs leading-5 text-muted">
                This clears the pending marker for this view. The original receipt stays unknown; a
                new action receives a new operation identifier.
              </p>
            </>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}
