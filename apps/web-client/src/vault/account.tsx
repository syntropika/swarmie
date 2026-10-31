import { useSource } from "@swarmie/client-ui";
import { Button, Input, Label, TextField } from "@swarmie/ui-web";
import { useState } from "react";

import { useClient } from "../client/context";

export function VaultSignIn() {
  const { vault } = useClient();
  const state = useSource(vault.state);
  if (state.phase !== "signed-out") {
    return null;
  }
  return (
    <form
      className="mt-6 space-y-4"
      onSubmit={(event) => {
        event.preventDefault();
        const form = event.currentTarget;
        const api = form.elements.namedItem("api");
        const authentication = form.elements.namedItem("authentication");
        if (api instanceof HTMLInputElement && authentication instanceof HTMLInputElement) {
          void vault
            .signIn({ api: api.value, authentication: authentication.value })
            .catch(() => false);
        }
      }}
    >
      <TextField name="api" type="url" isRequired defaultValue={state.cloud?.api ?? ""}>
        <Label>Cloud API URL</Label>
        <Input placeholder="https://your-cloud.example/" />
      </TextField>
      <TextField
        name="authentication"
        type="url"
        isRequired
        defaultValue={state.cloud?.authentication ?? ""}
      >
        <Label>Cloud authentication URL</Label>
        <Input placeholder="https://your-cloud.example/" />
      </TextField>
      <Button type="submit" isDisabled={state.signingIn}>
        {state.signingIn ? "Waiting for sign-in…" : "Sign in to Swarmie Cloud"}
      </Button>
    </form>
  );
}
export function VaultFounding() {
  const { vault } = useClient();
  const state = useSource(vault.state);
  const [confirm, setConfirm] = useState(false);
  if (!state.canFound) {
    return null;
  }
  const existing = state.current !== null;
  return (
    <div className="mt-6 space-y-3">
      <Button
        variant="secondary"
        onPress={() => {
          setConfirm(true);
        }}
      >
        {existing ? "Start a new empty vault" : "Create vault"}
      </Button>
      {confirm ? (
        <div className="space-y-3 rounded-xl border border-border p-4">
          <p className="text-xs leading-5">
            {existing
              ? "This starts a separate empty vault. It does not recover lost secrets or give new devices access to the old vault. Existing enrolled devices keep their own access to the archive."
              : "Create your personal encrypted vault. Losing every enrolled device makes its secrets unrecoverable."}
          </p>
          <div className="flex gap-2">
            <Button
              onPress={() => {
                void vault
                  .found(state.current)
                  .then((success) => {
                    if (success) {
                      setConfirm(false);
                    }
                  })
                  .catch(() => false);
              }}
            >
              Confirm and create
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
        </div>
      ) : null}
    </div>
  );
}
