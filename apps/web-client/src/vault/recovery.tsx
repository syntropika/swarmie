import { useSource } from "@swarmie/client-ui";
import type {
  CaptureSummary,
  MutationSummary,
  RecoveryRequest,
  SecretMetadata,
} from "@swarmie/plugin-vault-browser";
import { Button } from "@swarmie/ui-web";
import { useState } from "react";

import { useClient } from "../client/context";
import { VaultPager } from "./paging";
import { PolicyFields, readPolicy } from "./policy";

const scope = (value: SecretMetadata["projects"]) =>
  value.kind === "only" ? value.ids.join(", ") : "All";

function Policy({ metadata }: { metadata: SecretMetadata }) {
  return (
    <dl className="space-y-1 text-xs break-words">
      <dt>Name</dt>
      <dd>{metadata.name}</dd>
      <dt>Purpose</dt>
      <dd>{metadata.purpose || "None"}</dd>
      <dt>Projects</dt>
      <dd>{scope(metadata.projects)}</dd>
      <dt>Agents</dt>
      <dd>{scope(metadata.agents)}</dd>
      <dt>Approval</dt>
      <dd>{metadata.approval}</dd>
    </dl>
  );
}
function ConfirmAction({
  request,
  label,
  warning,
  metadata,
}: {
  request: RecoveryRequest;
  label: string;
  warning: string;
  metadata?: SecretMetadata;
}) {
  const { vault } = useClient();
  const [confirm, setConfirm] = useState(false);
  const [busy, setBusy] = useState(false);
  return (
    <div className="space-y-2">
      <Button
        variant="secondary"
        isDisabled={busy}
        onPress={() => {
          setConfirm(true);
        }}
      >
        {label}
      </Button>
      {confirm ? (
        <div className="space-y-3 rounded-xl border border-border p-3">
          <p className="text-xs leading-5">{warning}</p>
          {metadata ? <Policy metadata={metadata} /> : null}
          <div className="flex gap-2">
            <Button
              isDisabled={busy}
              onPress={() => {
                setBusy(true);
                void vault
                  .recovery(request)
                  .then((success) => {
                    if (success) {
                      setConfirm(false);
                    }
                  })
                  .finally(() => {
                    setBusy(false);
                  })
                  .catch(() => false);
              }}
            >
              Confirm
            </Button>
            <Button
              variant="tertiary"
              isDisabled={busy}
              onPress={() => {
                setConfirm(false);
              }}
            >
              Cancel
            </Button>
          </div>
        </div>
      ) : null}
    </div>
  );
}
/** Editing metadata never retrieves or repeats the retained secret's producer. */
function RetainedPolicy({
  metadata,
  label,
  request,
  warning,
}: {
  metadata: SecretMetadata;
  label: string;
  request: (policy: SecretMetadata) => RecoveryRequest;
  warning: string;
}) {
  const [editing, setEditing] = useState(false);
  const [review, setReview] = useState<SecretMetadata | null>(null);
  return (
    <div className="space-y-3">
      <Button
        variant="secondary"
        onPress={() => {
          setEditing(true);
          setReview(null);
        }}
      >
        {label}
      </Button>
      {editing ? (
        <form
          className="space-y-3 rounded-xl border border-border p-3"
          onSubmit={(event) => {
            event.preventDefault();
            const policy = readPolicy(event.currentTarget, metadata);
            if (policy) {
              setReview(policy);
              setEditing(false);
            }
          }}
        >
          <p className="text-xs">
            Change the name to resolve a name conflict, or adjust the policy. The retained value
            stays private.
          </p>
          <PolicyFields metadata={metadata} />
          <Button type="submit">Review retained value policy</Button>
          <Button
            variant="tertiary"
            onPress={() => {
              setEditing(false);
            }}
          >
            Cancel
          </Button>
        </form>
      ) : null}
      {review ? (
        <ConfirmAction
          label="Continue with reviewed policy"
          request={request(review)}
          metadata={review}
          warning={warning}
        />
      ) : null}
    </div>
  );
}
function Draft({ draft, outcome }: { draft: MutationSummary; outcome: string }) {
  const { vault } = useClient();
  const state = useSource(vault.state);
  const { latest } = draft;
  return (
    <div className="space-y-3 rounded-xl border border-border p-4">
      <p className="text-sm">{draft.metadata.name}</p>
      <p className="text-xs text-muted">
        Save refused: {outcome}. The encrypted draft is retained.
      </p>
      {!state.archived && !state.exposed ? (
        <RetainedPolicy
          label="Save retained draft against current version"
          metadata={draft.metadata}
          request={(metadata) => ({
            kind: "retry_draft",
            mutation: draft.prepared_entry,
            metadata,
            expected: latest,
          })}
          warning={`This uses the retained value privately, with the displayed policy, and replaces ${latest === null ? "no existing value" : `version ${latest}`}. It creates a new operation and resets previous use approvals.`}
        />
      ) : null}
      <ConfirmAction
        label="Discard draft"
        request={{ kind: "discard_draft", mutation: draft.prepared_entry }}
        warning="This permanently removes the retained encrypted draft from this node. It does not revoke a credential at its issuer."
      />
      <Button
        variant="tertiary"
        onPress={() => {
          void vault.synchronize().catch(() => false);
        }}
      >
        Refresh current version
      </Button>
    </div>
  );
}
const captureStatus = new Map<string, string>(
  Object.entries({
    awaiting_approval: "Waiting for approval",
    admitted: "Approved; preparing dispatch",
    dispatched: "Issuance in progress",
    candidate_held: "Ready to save",
    submitted: "Awaiting save result",
    saved: "Saved",
    cancelled: "Cancelled",
    producer_failed_not_issued: "No credential was issued",
    issued_unknown: "Issuance outcome unknown",
    candidate_lost: "Produced value unavailable",
    pending_store: "Save outcome unknown",
    needs_decision: "Review retained secret",
    stranded: "Vault is no longer writable",
    discarded: "Discarded",
  }),
);
function Capture({ capture }: { capture: CaptureSummary }) {
  const { vault } = useClient();
  const state = useSource(vault.state);
  const { latest } = capture;
  const action = (kind: "recognize" | "fence" | "store" | "discard"): RecoveryRequest => ({
    kind: "capture",
    capture: capture.id,
    action: { kind },
  });
  return (
    <div className="space-y-3 rounded-xl border border-border p-4">
      <h3 className="text-sm">{capture.metadata.name}</h3>
      <p className="text-xs text-foreground">
        {captureStatus.get(capture.state) ?? "Status unavailable"}
      </p>
      {capture.has_submission ? (
        <>
          <Button
            variant="tertiary"
            onPress={() => {
              void vault.recovery(action("recognize")).catch(() => false);
            }}
          >
            Check original save
          </Button>
          <ConfirmAction
            label="Settle original save"
            request={action("fence")}
            warning="Cloud will recognize the original immutable save or atomically prevent it from being dispatched. This does not repeat credential generation."
          />
        </>
      ) : null}
      {capture.has_candidate &&
      capture.state === "candidate_held" &&
      !state.archived &&
      !state.exposed ? (
        <ConfirmAction
          label="Save held secret"
          metadata={capture.metadata}
          request={action("store")}
          warning="Save the retained encrypted candidate against its original expected version. The secret is not shown and its producer is not repeated."
        />
      ) : null}
      {capture.has_candidate &&
      ["needs_decision", "stranded", "candidate_held", "pending_store"].includes(capture.state) &&
      !state.archived &&
      !state.exposed ? (
        <RetainedPolicy
          label="Rebase retained secret"
          metadata={capture.metadata}
          request={(metadata) => ({
            kind: "capture",
            capture: capture.id,
            action: { kind: "rebase", metadata, expected: latest },
          })}
          warning={`The original save must first be settled. This prepares the same held value against ${latest === null ? "no existing value" : `version ${latest}`}, resets prior use approvals, and still needs Save held secret. It never repeats issuance.`}
        />
      ) : null}
      {["saved", "discarded", "cancelled", "dispatched", "submitted"].includes(
        capture.state,
      ) ? null : (
        <ConfirmAction
          label="Discard retained capture"
          request={action("discard")}
          warning="The credential may still be live at its issuer. Discarding storage does not revoke it. An uncertain original save must be settled first."
        />
      )}
    </div>
  );
}
export function VaultRecovery() {
  const { vault } = useClient();
  const state = useSource(vault.state);
  return (
    <section className="mt-8 space-y-4">
      {state.pendingFoundings.map((pending) => (
        <div key={pending.operation} className="space-y-3 rounded-xl border border-border p-4">
          <output className="text-xs">
            Vault creation {pending.operation} has an uncertain result.
          </output>
          <Button
            variant="secondary"
            onPress={() => {
              void vault.resolveFounding(pending.vault).catch(() => false);
            }}
          >
            Recognize or prevent original creation
          </Button>
        </div>
      ))}
      {state.phase === "ready" &&
      (state.mutations.length || state.rejected.length || state.captures.length) ? (
        <h2 className="text-sm font-medium">Pending operations</h2>
      ) : null}
      {state.mutations.map((mutation) => (
        <div
          key={mutation.prepared_entry}
          className="space-y-3 rounded-xl border border-border p-4"
        >
          <p className="text-sm">{mutation.metadata.name}</p>
          <p className="text-xs text-muted">
            The save has not yet been reconciled. Do not submit it again.
          </p>
          <Button
            variant="tertiary"
            onPress={() => {
              void vault
                .recovery({
                  kind: "recognize_mutation",
                  mutation: mutation.prepared_entry,
                })
                .catch(() => false);
            }}
          >
            Check save
          </Button>
          <ConfirmAction
            label="Settle original save"
            request={{ kind: "fence_mutation", mutation: mutation.prepared_entry }}
            warning="Cloud will recognize the exact original save or atomically prevent it from being dispatched. A refused value stays as an encrypted draft."
          />
        </div>
      ))}
      {state.rejected.map(([draft, outcome]) => (
        <Draft key={draft.prepared_entry} draft={draft} outcome={outcome} />
      ))}
      {state.captures
        .filter((capture) => !["saved", "discarded", "cancelled"].includes(capture.state))
        .map((capture) => (
          <Capture key={capture.id} capture={capture} />
        ))}
      <VaultPager section="mutations" />
      <VaultPager section="rejected" />
      <VaultPager section="captures" />
    </section>
  );
}
