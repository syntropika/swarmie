import { useSource, errorMessage } from "@swarmie/client-ui";
import { Button, Icon, Link, Spinner, Surface, copyText } from "@swarmie/ui-web";
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
  onCancel,
}: {
  session: ProviderAuthorizationSession;
  provider: ProviderChoice;
  onConnected: (account: ProviderAccount) => void;
  onCancel: () => void;
}) {
  const state = useSource(session);
  const delivered = useRef<string | null>(null);
  const [copied, setCopied] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);
  const [retrying, setRetrying] = useState(false);
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
      <div className="grid gap-3">
        <p role="alert" className="text-sm text-danger">
          {state.error}
        </p>
        {state.retryable === true && session.retry !== undefined ? (
          <Button
            variant="secondary"
            isPending={retrying}
            isDisabled={retrying}
            onPress={() => {
              async function retry() {
                setRetrying(true);
                try {
                  await session.retry?.();
                } catch (error: unknown) {
                  failed(error);
                } finally {
                  setRetrying(false);
                }
              }
              retry().catch(failed);
            }}
          >
            Retry connection
          </Button>
        ) : null}
        {failure === null ? null : (
          <p role="alert" className="text-sm text-danger">
            {failure}
          </p>
        )}
        <Button variant="tertiary" onPress={onCancel}>
          {state.retryable === true ? "Cancel sign-in" : "Try a new sign-in"}
        </Button>
      </div>
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
    <div className="grid gap-4">
      <Surface variant="secondary">
        <div className="grid gap-4 p-4">
          {state.userCode === null ? null : (
            <>
              <p className="text-sm text-muted">Enter this code on the provider sign-in page:</p>
              <div className="flex min-w-0 items-center justify-between gap-3">
                <code className="min-w-0 text-xl font-medium tracking-widest break-all select-all">
                  {state.userCode}
                </code>
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
          <output className="flex items-start gap-2 text-sm text-muted">
            <Spinner size="sm" color="current" />
            Waiting for authorization. Return here after signing in.
          </output>
        </div>
      </Surface>
      {state.callbackUri === null ? null : (
        <OAuthCallback callbackUri={state.callbackUri} onComplete={session.complete} />
      )}
      {failure === null ? null : (
        <p role="alert" className="text-sm text-danger">
          {failure}
        </p>
      )}
      <Button variant="tertiary" onPress={onCancel}>
        Cancel sign-in
      </Button>
    </div>
  );
}

/** Browser authorization presentation; the node adapter supplies the subscribed session. */
export function OAuthFlow({
  provider,
  method,
  api,
  onConnected,
  accountId,
}: {
  provider: ProviderChoice;
  method: string;
  api: ProviderAuthorizationApi;
  onConnected: (account: ProviderAccount) => void;
  accountId?: string | undefined;
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
      const authorization = await api.begin(
        { providerId: provider.id, method, ...(accountId === undefined ? {} : { accountId }) },
        scope.signal,
      );
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
      try {
        await session.cancel();
      } finally {
        session.dispose();
        sessionRef.current = null;
        setSession(null);
      }
    }
  }
  return (
    <div className="grid gap-5">
      <p className="text-sm text-muted">
        Authorize your account in the browser. The execution node manages the connection.
      </p>
      {session === null ? (
        <Button
          isPending={busy}
          isDisabled={busy}
          onPress={() => {
            const page = window.open("about:blank", "_blank");
            if (page !== null) {
              page.opener = null;
            }
            begin(page).catch(failed);
          }}
        >
          {busy ? <Spinner size="sm" color="current" aria-hidden="true" /> : null}
          {busy ? `Signing in to ${provider.label}…` : `Sign in to ${provider.label}`}
        </Button>
      ) : (
        <AuthorizationSession
          session={session}
          provider={provider}
          onConnected={onConnected}
          onCancel={() => {
            cancel().catch(failed);
          }}
        />
      )}
      {failure === null ? null : (
        <p role="alert" className="text-sm text-danger">
          {failure}
        </p>
      )}
    </div>
  );
}
