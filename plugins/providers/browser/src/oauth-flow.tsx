import { useSource, errorMessage } from "@swarmie/client-ui";
import { Button, Icon, Link, Spinner, copyText } from "@swarmie/ui-web";
import { useEffect, useMemo, useRef, useState } from "react";

import type {
  ProviderAccount,
  ProviderAuthorizationSession,
  ProviderChoice,
  ProviderAuthorizationApi,
} from "./index";
import { OAuthCallback } from "./oauth-callback";

function AuthorizationSession({
  session,
  provider,
  onConnected,
}: {
  session: ProviderAuthorizationSession;
  provider: ProviderChoice;
  onConnected: (account: ProviderAccount) => void;
}) {
  const state = useSource(session);
  const delivered = useRef<string | null>(null);
  const [copied, setCopied] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);
  useEffect(() => {
    if (state.kind === "connected" && delivered.current !== state.account.id) {
      delivered.current = state.account.id;
      onConnected(state.account);
    }
  }, [state, onConnected]);
  function failed(error: unknown) {
    setFailure(errorMessage(error));
  }
  async function copyCode() {
    if (state.kind !== "waiting" || state.userCode === null) {
      return;
    }
    await copyText(state.userCode);
    setCopied(true);
  }
  if (state.kind === "failed") {
    return (
      <p role="alert" className="text-sm text-danger">
        {state.error}
      </p>
    );
  }
  if (state.kind === "connected") {
    return (
      <output className="flex items-center gap-2 text-sm">
        <Icon name="check" />
        Your account is connected.
      </output>
    );
  }
  return (
    <div className="grid gap-4 rounded-xl border border-border p-4">
      {state.userCode === null ? null : (
        <>
          <p className="text-sm text-muted">Enter this code on the provider sign-in page:</p>
          <div className="flex items-center justify-between gap-3">
            <code className="text-xl font-medium tracking-widest select-all">{state.userCode}</code>
            <Button
              variant="tertiary"
              isIconOnly
              aria-label={copied ? "Code copied" : "Copy sign-in code"}
              onPress={() => {
                copyCode().catch(failed);
              }}
            >
              <Icon name={copied ? "check" : "copy"} />
            </Button>
          </div>
        </>
      )}
      <Link href={state.authorizationUrl} target="_blank" rel="noopener noreferrer">
        Continue to {provider.label}
        <Link.Icon />
      </Link>
      <output className="flex items-center gap-2 text-sm text-muted">
        <Spinner size="sm" color="current" />
        Waiting for authorization. Return here after signing in.
      </output>
      {state.callbackUri === null ? null : (
        <OAuthCallback callbackUri={state.callbackUri} onComplete={session.complete} />
      )}
      {failure === null ? null : (
        <p role="alert" className="text-sm text-danger">
          {failure}
        </p>
      )}
    </div>
  );
}

/** Browser authorization presentation; the node adapter supplies the subscribed session. */
export function OAuthFlow({
  provider,
  method,
  api,
  onConnected,
}: {
  provider: ProviderChoice;
  method: string;
  api: ProviderAuthorizationApi;
  onConnected: (account: ProviderAccount) => void;
}) {
  const [session, setSession] = useState<ProviderAuthorizationSession | null>(null);
  const sessionRef = useRef<ProviderAuthorizationSession | null>(null);
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);
  const scope = useMemo(() => new AbortController(), []);
  useEffect(
    () => () => {
      scope.abort();
      sessionRef.current?.dispose();
    },
    [scope],
  );
  function failed(error: unknown) {
    setFailure(errorMessage(error));
  }
  async function begin(page: Window | null) {
    if (busy) {
      page?.close();
      return;
    }
    setBusy(true);
    setFailure(null);
    try {
      const authorization = await api.begin({ providerId: provider.id, method }, scope.signal);
      if (scope.signal.aborted) {
        authorization.dispose();
        page?.close();
        return;
      }
      sessionRef.current = authorization;
      setSession(authorization);
      const state = authorization.getSnapshot();
      if (state.kind === "waiting") {
        const url = new URL(state.authorizationUrl);
        if (url.protocol !== "https:" && url.protocol !== "http:") {
          throw new Error("The provider returned an invalid sign-in URL.");
        }
        if (page !== null && !page.closed) {
          page.location.replace(url.href);
        }
      } else {
        page?.close();
      }
    } catch (error: unknown) {
      page?.close();
      failed(error);
    } finally {
      if (!scope.signal.aborted) {
        setBusy(false);
      }
    }
  }
  async function cancel() {
    if (session !== null) {
      await session.cancel();
      session.dispose();
      sessionRef.current = null;
      setSession(null);
    }
  }
  return (
    <div className="grid gap-5">
      <p className="text-sm text-muted">
        Authorize your account in the browser. The execution node manages the connection.
      </p>
      {session === null ? (
        <Button
          tone="brand"
          shape="panel"
          size="lg"
          loading={busy}
          isDisabled={busy}
          onPress={() => {
            const page = window.open("about:blank", "_blank");
            if (page !== null) {
              page.opener = null;
            }
            begin(page).catch(failed);
          }}
        >
          Sign in to {provider.label}
        </Button>
      ) : (
        <>
          <AuthorizationSession session={session} provider={provider} onConnected={onConnected} />
          <Button
            variant="tertiary"
            onPress={() => {
              cancel().catch(failed);
            }}
          >
            Cancel sign-in
          </Button>
        </>
      )}
      {failure === null ? null : (
        <p role="alert" className="text-sm text-danger">
          {failure}
        </p>
      )}
    </div>
  );
}
