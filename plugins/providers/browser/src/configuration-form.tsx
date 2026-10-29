import { errorMessage } from "@swarmie/client-ui";
import { Button, Input, Label, Spinner, TextField } from "@swarmie/ui-web";
import { useState } from "react";

import type {
  ProviderAccount,
  ProviderConfigurationApi,
  ProviderConfigurationMethod,
  ProviderDiscoveryApi,
} from "./index";

function DetectServers({
  api,
  busy,
  onDetected,
  onFailure,
}: {
  api: ProviderDiscoveryApi;
  busy: boolean;
  onDetected: (url: string) => void;
  onFailure: (error: unknown) => void;
}) {
  const [detecting, setDetecting] = useState(false);
  async function detect() {
    setDetecting(true);
    try {
      const [server] = await api.detect(new AbortController().signal);
      if (server === undefined) {
        throw new Error("No local model server was found.");
      }
      onDetected(server);
    } finally {
      setDetecting(false);
    }
  }
  return (
    <Button
      variant="secondary"
      isDisabled={busy || detecting}
      isPending={detecting}
      onPress={() => {
        detect().catch(onFailure);
      }}
    >
      {detecting ? <Spinner size="sm" color="current" aria-hidden="true" /> : null}
      {detecting ? "Detecting servers…" : "Detect servers"}
    </Button>
  );
}

/** Configuration is resolved by the parent; this form receives its complete contract. */
export function ProviderConfigurationForm({
  api,
  discovery,
  providerId,
  method,
  onConfigured,
  onBusyChange,
}: {
  api: ProviderConfigurationApi;
  discovery: ProviderDiscoveryApi | null;
  providerId: string;
  method: ProviderConfigurationMethod;
  onConfigured: (account: ProviderAccount) => void;
  onBusyChange: (busy: boolean) => void;
}) {
  const [credential, setCredential] = useState("");
  const [url, setUrl] = useState("http://localhost:11434");
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);
  function failed(error: unknown) {
    setFailure(errorMessage(error));
  }
  async function configure() {
    if (busy) {
      return;
    }
    setBusy(true);
    onBusyChange(true);
    setFailure(null);
    try {
      const account = await api.configure(
        {
          providerId,
          method: method.id,
          ...(method.kind === "local" ? { url } : { credential }),
        },
        new AbortController().signal,
      );
      setCredential("");
      onConfigured(account);
    } finally {
      setBusy(false);
      onBusyChange(false);
    }
  }
  return (
    <div className="grid gap-5">
      {method.kind === "local" ? (
        <>
          <TextField type="url" value={url} onChange={setUrl}>
            <Label>Server URL</Label>
            <Input fullWidth />
          </TextField>
          {discovery === null ? null : (
            <DetectServers api={discovery} busy={busy} onDetected={setUrl} onFailure={failed} />
          )}
        </>
      ) : (
        <TextField type="password" autoComplete="off" value={credential} onChange={setCredential}>
          <Label>API key</Label>
          <Input fullWidth />
        </TextField>
      )}
      {failure === null ? null : (
        <p role="alert" className="text-sm text-danger">
          {failure}
        </p>
      )}
      <p className="text-xs text-muted">Choose your model later.</p>
      <div className="flex justify-end border-t border-border pt-5">
        <Button
          isPending={busy}
          isDisabled={busy || (method.kind === "api-key" && credential.trim() === "")}
          onPress={() => {
            configure().catch(failed);
          }}
        >
          {busy ? <Spinner size="sm" color="current" aria-hidden="true" /> : null}
          {busy ? "Connecting provider…" : "Connect provider"}
        </Button>
      </div>
    </div>
  );
}
