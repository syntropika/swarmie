import { useSource } from "@swarmie/client-ui";
import { readEnrollmentRequest, readJoinProposal } from "@swarmie/plugin-vault-browser";
import type { VaultIdentity } from "@swarmie/plugin-vault-browser";
import { Button, Input, Label, TextArea, TextField } from "@swarmie/ui-web";
import { useState } from "react";
import type { SubmitEvent } from "react";

import { useClient } from "../client/context";
import { VaultPager } from "./paging";

function VerifiedTransfer({
  label,
  submit,
}: {
  label: string;
  submit: (payload: string, code: string) => Promise<boolean>;
}) {
  const [error, setError] = useState(false);
  const [busy, setBusy] = useState(false);
  async function send(event: SubmitEvent<HTMLFormElement>) {
    event.preventDefault();
    if (busy) {
      return;
    }
    const form = event.currentTarget;
    const payload = form.elements.namedItem("payload");
    const code = form.elements.namedItem("code");
    if (!(payload instanceof HTMLTextAreaElement) || !(code instanceof HTMLInputElement)) {
      return;
    }
    setBusy(true);
    setError(false);
    try {
      if (await submit(payload.value, code.value.trim())) {
        form.reset();
      } else {
        setError(true);
      }
    } catch {
      setError(true);
    } finally {
      setBusy(false);
    }
  }
  return (
    <form
      className="mt-4 space-y-3"
      onSubmit={(event) => {
        void send(event).catch(() => false);
      }}
    >
      <TextField name="payload" isRequired>
        <Label>{label}</Label>
        <TextArea rows={4} maxLength={32_768} />
      </TextField>
      <TextField name="code" isRequired>
        <Label>Full verification code from the other device</Label>
        <Input minLength={64} maxLength={64} pattern="[a-f0-9]{64}" />
      </TextField>
      <p className="text-xs text-foreground">
        Compare the complete code through an independent channel. Cloud discovery alone cannot
        establish trust.
      </p>
      <Button type="submit" isDisabled={busy}>
        Confirm compared code
      </Button>
      {error ? (
        <p role="alert" className="text-xs text-danger">
          The transfer could not be confirmed. Check both devices and the complete code.
        </p>
      ) : null}
    </form>
  );
}
function PublicTransfer({ value, code }: { value: unknown; code: string }) {
  return (
    <div className="space-y-2">
      <TextField isReadOnly value={JSON.stringify(value)}>
        <Label>Public transfer data</Label>
        <TextArea rows={4} />
      </TextField>
      <p className="font-mono text-xs break-all">{code}</p>
    </div>
  );
}
function CompareOffer({ id }: { id: string }) {
  const { vault } = useClient();
  return (
    <form
      className="mt-3 flex flex-wrap gap-2"
      onSubmit={(event) => {
        event.preventDefault();
        const code = event.currentTarget.elements.namedItem("code");
        if (code instanceof HTMLInputElement) {
          void vault
            .lifecycle({
              kind: "approve",
              offer: id,
              verified_proposal_code: code.value.trim(),
            })
            .catch(() => false);
        }
      }}
    >
      <TextField name="code" isRequired>
        <Label>Code compared on the recipient</Label>
        <Input minLength={64} maxLength={64} pattern="[a-f0-9]{64}" />
      </TextField>
      <Button type="submit">Approve device access</Button>
      <Button
        variant="tertiary"
        onPress={() => {
          void vault.lifecycle({ kind: "cancel_offer", offer: id }).catch(() => false);
        }}
      >
        Cancel
      </Button>
    </form>
  );
}
function ReplacementEnrollment({ identity }: { identity: VaultIdentity }) {
  const { vault } = useClient();
  const [confirm, setConfirm] = useState(false);
  return (
    <div className="space-y-2">
      <Button
        variant="tertiary"
        onPress={() => {
          setConfirm(true);
        }}
      >
        Enroll a replacement identity
      </Button>
      {confirm ? (
        <div className="space-y-2 rounded-xl border border-border p-3">
          <p className="text-xs">
            Use this if the local device keys are unavailable. An existing enrolled device must
            approve the new identity. Previous keys, encrypted journals and Cloud enrollments are
            retained; retire the old device separately if needed.
          </p>
          <Button
            onPress={() => {
              void vault
                .lifecycle({ kind: "begin", identity })
                .then((success) => {
                  if (success) {
                    setConfirm(false);
                  }
                })
                .catch(() => false);
            }}
          >
            Begin replacement enrollment
          </Button>
          <Button
            variant="tertiary"
            onPress={() => {
              setConfirm(false);
            }}
          >
            Cancel
          </Button>
        </div>
      ) : null}
    </div>
  );
}
function DeviceRemoval({ target, identity }: { target: string; identity: VaultIdentity }) {
  const { vault } = useClient();
  const state = useSource(vault.state);
  const confirm = state.lifecycle.removal?.target === target ? state.lifecycle.removal : null;
  return (
    <div className="mt-3 space-y-2">
      <p className="font-mono text-xs break-all">{target}</p>
      <div className="flex flex-wrap gap-2">
        {[false, true].map((compromised) => (
          <Button
            key={String(compromised)}
            variant="tertiary"
            onPress={() => {
              void vault
                .lifecycle({ kind: "preview_removal", identity, target, compromised })
                .catch(() => false);
            }}
          >
            {compromised ? "Mark compromised" : "Retire device"}
          </Button>
        ))}
      </div>
      {confirm ? (
        <div className="space-y-2 rounded-xl border border-border p-3">
          <p className="text-xs">
            {confirm.compromised
              ? "This marks the vault exposed and removes the listed device and its admitted descendants. Rotate credentials at their issuers."
              : "This removes this device's access. It does not revoke credentials at their issuers."}
          </p>
          <ul className="space-y-1 text-xs">
            {confirm.affected.map((id) => (
              <li key={id} className="font-mono break-all">
                {id}
              </li>
            ))}
          </ul>
          <Button
            variant="danger"
            onPress={() => {
              void vault
                .lifecycle({
                  kind: "remove",
                  identity: confirm.identity,
                  target: confirm.target,
                  compromised: confirm.compromised,
                  affected: confirm.affected,
                  authority_base: confirm.authority_base,
                })
                .catch(() => false);
            }}
          >
            Confirm removal
          </Button>
        </div>
      ) : null}
    </div>
  );
}
function RestoreArchive({
  identity,
  current,
  exposed,
}: {
  identity: VaultIdentity;
  current: string;
  exposed: boolean;
}) {
  const { vault } = useClient();
  const [confirm, setConfirm] = useState(false);
  return (
    <div className="space-y-3">
      <Button
        variant="secondary"
        onPress={() => {
          setConfirm(true);
        }}
      >
        Restore archive as current
      </Button>
      {confirm ? (
        <div className="space-y-3 rounded-xl border border-border p-4">
          <p className="text-xs">
            This makes this archive current and archives the current vault. It does not merge their
            secrets or enroll other devices. The old device signing key is required; unlocking
            secret values is not required.
          </p>
          {exposed ? (
            <p role="alert" className="text-xs text-foreground">
              This vault is exposed. Restoring it preserves that warning and refuses new or
              replacement values. Rotate credentials at their issuers.
            </p>
          ) : null}
          <Button
            onPress={() => {
              void vault
                .lifecycle({ kind: "restore", identity, expected_current: current })
                .then((success) => {
                  if (success) {
                    setConfirm(false);
                  }
                })
                .catch(() => false);
            }}
          >
            Confirm restore
          </Button>
          <Button
            variant="tertiary"
            onPress={() => {
              setConfirm(false);
            }}
          >
            Cancel
          </Button>
        </div>
      ) : null}
    </div>
  );
}
export function VaultDevices() {
  const { vault } = useClient();
  const state = useSource(vault.state);
  if (state.phase === "unavailable" || state.phase === "signed-out") {
    return null;
  }
  return (
    <section className="mt-8 space-y-4">
      <h2 className="text-sm font-medium">Devices and vaults</h2>
      {state.lifecycle.roots.map((root) => (
        <div key={root.identity.vault} className="space-y-3 rounded-xl border border-border p-4">
          <p className="font-mono text-xs break-all">
            {root.identity.vault}
            {root.identity.vault === state.current ? " · Current" : " · Archived"}
            {root.exposed ? " · Exposed" : ""}
          </p>
          <div className="flex flex-wrap gap-2">
            {root.enrolled ? (
              <>
                <Button
                  variant="secondary"
                  onPress={() => {
                    void vault
                      .lifecycle({ kind: "select", identity: root.identity })
                      .catch(() => false);
                  }}
                >
                  Open vault
                </Button>
                <Button
                  variant="tertiary"
                  onPress={() => {
                    void vault
                      .lifecycle({ kind: "inspect", identity: root.identity })
                      .catch(() => false);
                  }}
                >
                  Manage device access
                </Button>
                <Button
                  variant="tertiary"
                  onPress={() => {
                    void vault
                      .lifecycle({ kind: "rebind", identity: root.identity })
                      .catch(() => false);
                  }}
                >
                  Bind device to this login
                </Button>
                {root.pendingReplacement === null ? (
                  <ReplacementEnrollment identity={root.identity} />
                ) : (
                  <div className="space-y-2">
                    <p className="text-xs">
                      Replacement identity awaiting approval from an existing device. The current
                      identity remains available.
                    </p>
                    <p className="font-mono text-xs break-all">
                      {root.pendingReplacement.verifiedCode}
                    </p>
                    <Button
                      variant="tertiary"
                      onPress={() => {
                        if (root.pendingReplacement) {
                          void vault
                            .lifecycle({
                              kind: "cancel_join",
                              identity: root.identity,
                              device: root.pendingReplacement.device,
                            })
                            .catch(() => false);
                        }
                      }}
                    >
                      Cancel replacement
                    </Button>
                  </div>
                )}
              </>
            ) : (
              <Button
                variant="secondary"
                onPress={() => {
                  void vault
                    .lifecycle({ kind: "begin", identity: root.identity })
                    .catch(() => false);
                }}
              >
                Enroll this device
              </Button>
            )}
          </div>
          {root.verifiedCode === null ? null : (
            <div className="space-y-1">
              <p className="text-xs">Enrollment code verified on this device</p>
              <p className="font-mono text-xs break-all">{root.verifiedCode}</p>
            </div>
          )}
          {root.certificates.length > 0
            ? root.certificates
                .filter((certificate) => !root.removed.includes(certificate.statement.recipient))
                .map((certificate) => (
                  <DeviceRemoval
                    key={certificate.statement.recipient}
                    target={certificate.statement.recipient}
                    identity={root.identity}
                  />
                ))
            : null}
          {root.enrolled && state.current !== null && root.identity.vault !== state.current ? (
            <RestoreArchive
              identity={root.identity}
              current={state.current}
              exposed={root.exposed}
            />
          ) : null}
        </div>
      ))}
      <VaultPager section="devices" />
      {state.lifecycle.joins.map((join) => (
        <div key={join.request.device} className="space-y-3 rounded-xl border border-border p-4">
          <h3 className="text-sm">Request access from an existing device</h3>
          {join.keysUnavailable === true ? (
            <p role="alert" className="text-xs">
              The provisional device keys are unavailable. Cancel this request and start a new
              enrollment.
            </p>
          ) : (
            <PublicTransfer value={join.request} code={join.code} />
          )}
          <Button
            variant="tertiary"
            onPress={() => {
              void vault
                .lifecycle({
                  kind: "cancel_join",
                  identity: join.request.identity,
                  device: join.request.device,
                })
                .catch(() => false);
            }}
          >
            Cancel enrollment
          </Button>
        </div>
      ))}
      {state.phase === "ready" ? (
        <VerifiedTransfer
          label="Request from the new device"
          submit={async (payload, code) =>
            await vault.lifecycle({
              kind: "prepare",
              request: readEnrollmentRequest(payload),
              verified_request_code: code,
            })
          }
        />
      ) : null}
      {state.lifecycle.offers.map((offer) => (
        <div key={offer.id} className="space-y-3 rounded-xl border border-border p-4">
          <h3 className="text-sm">Verify this proposal on the recipient</h3>
          <PublicTransfer value={offer.proposal} code={offer.code} />
          <CompareOffer id={offer.id} />
        </div>
      ))}
      <VaultPager section="offers" />
      {state.lifecycle.joins.length ? (
        <VerifiedTransfer
          label="Proposal from the existing device"
          submit={async (payload, code) =>
            await vault.lifecycle({
              kind: "accept",
              proposal: readJoinProposal(payload),
              verified_proposal_code: code,
            })
          }
        />
      ) : null}
      {state.lifecycle.pending.map((receipt) => (
        <div key={receipt.operation} className="space-y-2">
          <output className="text-xs">
            Device change {receipt.operation} has an uncertain result.
          </output>
          <Button
            variant="secondary"
            onPress={() => {
              void vault
                .lifecycle({ kind: "resolve", operation: receipt.operation })
                .catch(() => false);
            }}
          >
            Recognize or prevent original change
          </Button>
        </div>
      ))}
    </section>
  );
}
