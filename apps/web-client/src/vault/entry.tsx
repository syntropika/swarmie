import type { SecretSummary } from "@swarmie/plugin-vault-browser";
import { Button, Input, Label, TextField } from "@swarmie/ui-web";
import { useState } from "react";
import type { SubmitEvent } from "react";

import { useClient } from "../client/context";
import { PolicyFields, readPolicy } from "./policy";

export function EntryForm({ entry, close }: { entry: SecretSummary | null; close: () => void }) {
  const { vault } = useClient();
  const [busy, setBusy] = useState(false);
  async function save(event: SubmitEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = event.currentTarget;
    const secret = form.elements.namedItem("secret");
    if (!(secret instanceof HTMLInputElement) || !secret.value || busy) {
      return;
    }
    const metadata = readPolicy(
      form,
      entry?.metadata ?? {
        name: "",
        purpose: "",
        projects: { kind: "unrestricted" },
        agents: { kind: "unrestricted" },
        approval: "operation",
        policy_generation: "",
        target_account: null,
      },
    );
    if (!metadata) {
      return;
    }
    // Preserve a value-only replacement's policy generation and its existing grants.
    if (
      entry &&
      JSON.stringify({ ...entry.metadata, policy_generation: "" }) ===
        JSON.stringify({ ...metadata, policy_generation: "" })
    ) {
      metadata.policy_generation = entry.metadata.policy_generation;
    }
    setBusy(true);
    const pending = vault.save(
      metadata,
      secret.value,
      entry ? { reference: entry.reference, revision: entry.revision } : undefined,
    );
    secret.value = "";
    try {
      if (await pending) {
        close();
      }
    } finally {
      setBusy(false);
    }
  }

  return (
    <form
      onSubmit={(event) => {
        void save(event).catch(() => false);
      }}
      className="mt-6 space-y-4 rounded-xl border border-border p-4"
    >
      <PolicyFields
        metadata={
          entry?.metadata ?? {
            name: "",
            purpose: "",
            projects: { kind: "unrestricted" },
            agents: { kind: "unrestricted" },
            approval: "operation",
            policy_generation: "",
            target_account: null,
          }
        }
      />
      <TextField name="secret" type="password" isRequired autoComplete="off">
        <Label>{entry ? "Replacement value" : "Secret value"}</Label>
        <Input maxLength={65_536} />
      </TextField>
      <div className="flex gap-2">
        <Button type="submit" isDisabled={busy}>
          Save privately
        </Button>
        <Button variant="tertiary" isDisabled={busy} onPress={close}>
          Cancel
        </Button>
      </div>
    </form>
  );
}
export function Entry({ entry, edit }: { entry: SecretSummary; edit: () => void }) {
  const { vault } = useClient();
  const [confirm, setConfirm] = useState(false);
  return (
    <div className="flex flex-wrap items-center gap-3 border-b border-border py-4 last:border-0">
      <div className="min-w-0 flex-1">
        <h2 className="text-sm font-medium">{entry.metadata.name}</h2>
        <p className="mt-1 text-xs text-muted">{entry.metadata.purpose}</p>
        <p className="mt-1 font-mono text-xs break-all text-muted">{entry.reference}</p>
      </div>
      <Button variant="secondary" onPress={edit}>
        Replace
      </Button>
      <Button
        variant="tertiary"
        onPress={() => {
          setConfirm(true);
        }}
      >
        Delete
      </Button>
      {confirm ? (
        <div className="flex w-full items-center gap-2 text-xs">
          <span>Delete this secret?</span>
          <Button
            variant="danger"
            onPress={() => {
              void vault.remove(entry).catch(() => false);
              setConfirm(false);
            }}
          >
            Delete secret
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
