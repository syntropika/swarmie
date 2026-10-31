import { useSource } from "@swarmie/client-ui";
import type { SecretSummary, VaultState } from "@swarmie/plugin-vault-browser";
import { Button, Label, Switch } from "@swarmie/ui-web";
import { useState } from "react";

import { useClient } from "../client/context";
import { VaultFounding, VaultSignIn } from "./account";
import { VaultGlobalCards } from "./cards";
import { VaultDevices } from "./devices";
import { Entry, EntryForm } from "./entry";
import { VaultPager } from "./paging";
import { VaultRecovery } from "./recovery";

export function VaultSettings() {
  const { vault } = useClient();
  const state = useSource(vault.state);
  const [editing, setEditing] = useState<SecretSummary | null | undefined>();
  return (
    <>
      <h1 className="text-2xl font-medium">Vault</h1>
      <p className="mt-2 text-xs leading-5 text-muted">
        Your node stores encrypted secrets. Agents use references. Saved values can be replaced or
        deleted.
      </p>
      {state.phase === "unavailable" ? (
        <p className="mt-6 text-sm text-muted">
          The private native vault connection is unavailable in this client.
        </p>
      ) : null}
      <VaultSignIn />
      {state.phase === "locked" || state.phase === "unlocking" ? (
        <Button
          className="mt-6"
          isDisabled={state.phase === "unlocking"}
          onPress={() => {
            void vault.unlock().catch(() => false);
          }}
        >
          {state.phase === "unlocking" ? "Unlocking…" : "Unlock vault"}
        </Button>
      ) : null}
      {state.phase === "joining" ? (
        <p className="mt-6 text-sm">
          Approve this device from an existing enrolled device and compare its verification code.
        </p>
      ) : null}
      {state.phase === "ready" ? (
        <>
          <div className="mt-6 flex flex-wrap items-center gap-3">
            <output className="text-xs text-muted">
              {
                {
                  confirmed: "Synchronized",
                  synchronizing: "Synchronizing…",
                  offline: "Offline · last confirmed version",
                }[state.synchronization]
              }
            </output>
            <Button
              variant="tertiary"
              onPress={() => {
                void vault.synchronize().catch(() => false);
              }}
            >
              Synchronize
            </Button>
            <Button
              variant="tertiary"
              onPress={() => {
                void vault.signOut().catch(() => false);
              }}
            >
              Sign out
            </Button>
          </div>
          {state.archived ? (
            <output className="mt-4 text-xs">
              Archived vault. Existing secrets remain readable; creating or replacing values is
              unavailable.
            </output>
          ) : null}
          {state.exposed ? (
            <p role="alert" className="mt-4 text-xs text-danger">
              This vault is marked exposed. Rotate credentials at their issuers. New values cannot
              be stored here.
            </p>
          ) : null}
          {!state.archived && !state.exposed ? (
            <Button
              className="mt-5"
              onPress={() => {
                setEditing(null);
              }}
            >
              Add secret
            </Button>
          ) : null}
          {editing === undefined ? null : (
            <EntryForm
              entry={editing}
              close={() => {
                setEditing(undefined);
              }}
            />
          )}
          <div className="mt-5">
            {state.entries.map((entry) => (
              <Entry
                key={entry.reference}
                entry={entry}
                edit={() => {
                  setEditing(entry);
                }}
              />
            ))}
          </div>
        </>
      ) : null}
      <VaultFounding />
      {state.needsSignIn ? (
        <output className="mt-4 space-y-3">
          <p className="text-xs">
            Cloud sign-in needs renewal. The last unlocked version remains usable offline. Signing
            out will close vault sessions for this account.
          </p>
          <Button
            variant="secondary"
            onPress={() => {
              void vault.signOut().catch(() => false);
            }}
          >
            Sign out and sign in again
          </Button>
        </output>
      ) : null}
      {state.phase !== "signed-out" && state.phase !== "unavailable" && state.phase !== "ready" ? (
        <Button
          variant="tertiary"
          className="mt-4"
          onPress={() => {
            void vault.signOut().catch(() => false);
          }}
        >
          Sign out
        </Button>
      ) : null}
      <VaultDevices />
      <VaultRecovery />
      <VaultPager section="entries" />
      <VaultGlobalCards />
      <VaultPager section="cards" />
      {state.error === null ? null : (
        <p role="alert" className="mt-4 text-xs text-danger">
          {state.error}
        </p>
      )}
    </>
  );
}
function protectionStatus(
  available: boolean,
  state: Pick<VaultState, "detectorEnabled" | "detectorAvailable">,
): string {
  if (!available) {
    return "Native protection is unavailable in this client.";
  }
  if (!state.detectorEnabled) {
    return "Unprotected: content is sent without this detector.";
  }
  if (!state.detectorAvailable) {
    return "The enabled detector is unavailable. Sending requires an explicit exception.";
  }
  return "Possible credentials stop sending until you cancel, correct the content, or allow that exact attempt once.";
}
export function ModelProtectionSettings() {
  const { vault } = useClient();
  const state = useSource(vault.state);
  return (
    <div className="mt-6 space-y-4">
      <p className="text-xs leading-5 text-muted">
        This core plugin inspects the final text and JSON sent to a model. It starts enabled and
        does not require permission acceptance.
      </p>
      <Switch
        isSelected={state.detectorEnabled}
        isDisabled={!vault.available}
        onChange={(enabled) => {
          void vault.configureDetector(enabled).catch(() => false);
        }}
      >
        <Switch.Control>
          <Switch.Thumb />
        </Switch.Control>
        <Switch.Content>
          <Label>Detect possible credentials</Label>
        </Switch.Content>
      </Switch>
      <output className="text-xs text-muted">{protectionStatus(vault.available, state)}</output>
    </div>
  );
}
