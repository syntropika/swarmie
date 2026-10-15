import type { AccountSession, AccountSnapshot } from "@swarmie/contracts";
import { QueryClient, useInfiniteQuery, useMutation } from "@tanstack/react-query";
// Copyright 2026 Syntropika
// SPDX-License-Identifier: Apache-2.0
import { useEffect, useMemo, useRef, useState } from "react";

import { Button } from "@/components/ui/button";

import { useAccount, useAccountService } from "./account-state";
import { startAction, errorMessage, errorReporter } from "./action-boundary";
import { hasText } from "./value-presence";

export function AccountSecurity({ onProfile }: { onProfile: () => void }) {
  const { account, error: observationError } = useAccount();
  return (
    <AccountSecurityView
      key={account?.profile?.id ?? "signed-out"}
      account={account}
      observationError={observationError}
      onProfile={onProfile}
    />
  );
}
function AccountSecurityView({
  account,
  observationError,
  onProfile,
}: {
  account?: AccountSnapshot;
  observationError: string;
  onProfile: () => void;
}) {
  const service = useAccountService();
  const logoutMutation = useMutation({
    mutationFn: async (accountId: string) => {
      const result = await service.request("account/logout", { expectedAccountId: accountId });
      await service.reconcile();
      return result;
    },
    retry: false,
    gcTime: 0,
  });
  const busy = logoutMutation.isPending;
  const [message, setMessage] = useState("");
  const [actionError, setActionError] = useState("");
  const profile = account?.status === "signedIn" ? account.profile : null;
  const logout = async () => {
    if (!profile) {
      return;
    }
    setActionError("");
    setMessage("");
    try {
      const result = await logoutMutation.mutateAsync(profile.id);
      setMessage(
        result.remoteRevocationConfirmed
          ? "This installation is signed out."
          : "Signed out locally. Remote revocation could not be confirmed.",
      );
    } catch (error) {
      setActionError(errorMessage(error));
    }
  };
  return (
    <section className="grid gap-5" aria-label="Account security">
      {(actionError || observationError) && (
        <p role="alert" className="text-sm text-danger">
          {actionError || observationError}
        </p>
      )}
      {message && <output className="text-muted text-sm">{message}</output>}
      {profile ? (
        <>
          <dl className="grid gap-2 text-sm">
            <dt className="text-muted">Signed in as</dt>
            <dd className="break-all">{profile.email}</dd>
            <dt className="text-muted mt-2">Login storage</dt>
            <dd>
              {account?.persistence === "session"
                ? "For this owner session only"
                : "Protected by your system credential vault"}
            </dd>
          </dl>
          <p className="text-muted text-sm leading-relaxed">
            Closing Swarmie keeps this login available. Signing out removes it from this
            installation and asks the account service to revoke its access. Your browser login has
            its own session.
          </p>
          <div>
            <Button
              variant="outline"
              disabled={busy}
              onClick={() => {
                startAction(logout(), errorReporter(setActionError));
              }}
            >
              {busy ? "Signing out…" : "Sign out of this installation"}
            </Button>
          </div>
          <NativeSessions
            key={profile.id}
            accountId={profile.id}
            onMessage={setMessage}
            onError={setActionError}
          />
        </>
      ) : (
        <>
          <p className="text-muted text-sm">
            {account
              ? "Sign in to review this installation’s account session."
              : "Checking your account…"}
          </p>
          <div>
            <Button variant="outline" onClick={onProfile}>
              Open account
            </Button>
          </div>
        </>
      )}
    </section>
  );
}

function date(value: string) {
  const parsed = new Date(value);
  return Number.isNaN(parsed.getTime())
    ? "Unavailable"
    : parsed.toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
}
function SessionDetails({ session }: { session: AccountSession }) {
  return (
    <dl className="text-muted grid gap-1 text-sm">
      <div>
        <dt className="inline">Signed in </dt>
        <dd className="inline">
          <time dateTime={session.authenticatedAt}>{date(session.authenticatedAt)}</time>
        </dd>
      </div>
      <div>
        <dt className="inline">Last active </dt>
        <dd className="inline">
          <time dateTime={session.lastUsedAt}>{date(session.lastUsedAt)}</time>
        </dd>
      </div>
      <div>
        <dt className="inline">Expires </dt>
        <dd className="inline">
          <time dateTime={session.expiresAt}>{date(session.expiresAt)}</time>
        </dd>
      </div>
    </dl>
  );
}
function SessionFailure({
  message,
  onRefresh,
  disabled,
}: {
  message: string;
  onRefresh: () => void;
  disabled: boolean;
}) {
  const recovery = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (!disabled) {
      recovery.current?.focus();
    }
  }, [disabled]);
  return (
    <div className="grid gap-2">
      <p role="alert" className="text-sm text-danger">
        {message} Refresh the session state before trying again.
      </p>
      <div>
        <Button ref={recovery} variant="ghost" disabled={disabled} onClick={onRefresh}>
          Refresh session state
        </Button>
      </div>
    </div>
  );
}
function NativeSessions({
  accountId,
  onMessage,
  onError,
}: {
  accountId: string;
  onMessage: (value: string) => void;
  onError: (value: string) => void;
}) {
  const service = useAccountService();
  // A keyed component owns a private account cache; disposal clears principal-specific data.
  const queryClient = useMemo(
    () => new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0 } } }),
    [],
  );
  const disposalGeneration = useRef(0);
  useEffect(() => {
    disposalGeneration.current += 1;
    return () => {
      disposalGeneration.current += 1;
      const disposedGeneration = disposalGeneration.current;
      queueMicrotask(() => {
        if (disposalGeneration.current !== disposedGeneration) {
          return;
        }
        startAction(queryClient.cancelQueries(), errorReporter(onError));
        queryClient.clear();
      });
    };
  }, [queryClient, onError]);
  const sessions = useInfiniteQuery(
    {
      queryKey: ["account-sessions", accountId],
      initialPageParam: "",
      queryFn: async ({ pageParam }) =>
        await service.request("account/sessions/read", {
          expectedAccountId: accountId,
          ...(hasText(pageParam) ? { after: pageParam } : {}),
        }),
      getNextPageParam: (last) => last.nextCursor ?? undefined,
      refetchOnWindowFocus: false,
    },
    queryClient,
  );
  const page = sessions.data?.pages[0];
  const others =
    sessions.data?.pages
      .flatMap((value) => value.others)
      .filter((session, index, all) => all.findIndex((item) => item.id === session.id) === index) ??
    [];
  const mutation = useMutation(
    {
      mutationFn: async ({ action }: { id: string; action: () => Promise<unknown> }) => {
        try {
          return await action();
        } catch (error) {
          await service.reconcile();
          throw error;
        }
      },
      retry: false,
      gcTime: 0,
    },
    queryClient,
  );
  const busy = mutation.isPending || sessions.isFetching;
  const busyId = mutation.isPending ? mutation.variables.id : "";
  const [confirmAll, setConfirmAll] = useState(false);
  const cancelConfirmation = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (confirmAll) {
      cancelConfirmation.current?.focus();
    }
  }, [confirmAll]);
  const run = (id: string, action: () => Promise<unknown>) => {
    onError("");
    onMessage("");
    startAction(mutation.mutateAsync({ id, action }), errorReporter(onError));
  };
  const load = async () => {
    await sessions.refetch({ throwOnError: true });
  };
  const revoke = (session: AccountSession) => {
    run(session.id, async () => {
      await service.request("account/sessions/revoke", {
        expectedAccountId: accountId,
        sessionId: session.id,
      });
      onMessage("The selected native session is signed out.");
      await load();
    });
  };
  const revokeAll = () => {
    run("all", async () => {
      await service.request("account/sessions/revoke-all", { expectedAccountId: accountId });
      onMessage("All native sessions are signed out. Your browser session remains available.");
      await service.reconcile();
    });
  };
  const failure = (() => {
    if (sessions.error) {
      return { id: "read", message: errorMessage(sessions.error) };
    }
    if (mutation.error) {
      return { id: mutation.variables.id, message: errorMessage(mutation.error) };
    }
    return null;
  })();
  const feedback = (id: string) =>
    failure?.id === id ? (
      <SessionFailure
        message={failure.message}
        disabled={busy}
        onRefresh={() => {
          run("refresh", load);
        }}
      />
    ) : null;
  return (
    <section
      className="grid min-w-0 gap-4 border-t border-border pt-6"
      aria-label="Native sessions"
      aria-busy={busy}
    >
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h3 className="text-sm font-semibold">Native sessions</h3>
        <Button
          variant="ghost"
          disabled={busy}
          onClick={() => {
            run("refresh", load);
          }}
        >
          {busyId === "refresh" ? "Refreshing…" : "Refresh sessions"}
        </Button>
      </div>
      <p className="text-muted text-sm leading-relaxed">
        These sessions connect Swarmie installations to your account. Activity times can lag by five
        minutes. Your browser session is managed separately.
      </p>
      {feedback("read")}
      {feedback("refresh")}
      {sessions.isPending && <output className="text-muted text-sm">Loading sessions…</output>}
      {page && (
        <>
          <div className="grid gap-2">
            <h4 className="text-sm font-medium">This installation</h4>
            <SessionDetails session={page.current} />
          </div>
          <div className="grid gap-2">
            <h4 className="text-sm font-medium">Other native sessions</h4>
            {others.length > 0 ? (
              <ul className="grid divide-y divide-border">
                {others.map((session) => (
                  <li key={session.id} className="grid min-w-0 gap-3 py-4">
                    <div className="flex flex-wrap items-center justify-between gap-2">
                      <span className="text-sm font-medium">Session {session.id.slice(0, 8)}</span>
                      <Button
                        variant="outline"
                        aria-label={`Sign out session ${session.id.slice(0, 8)}`}
                        disabled={busy}
                        onClick={() => {
                          revoke(session);
                        }}
                      >
                        {busyId === session.id ? "Signing out…" : "Sign out"}
                      </Button>
                    </div>
                    {feedback(session.id)}
                    <SessionDetails session={session} />
                    <p className="text-muted text-xs break-all">
                      Installation {session.installationId}
                    </p>
                  </li>
                ))}
              </ul>
            ) : (
              <p className="text-muted text-sm">No other active native sessions.</p>
            )}
            {sessions.hasNextPage && (
              <div>
                <Button
                  variant="outline"
                  disabled={busy}
                  onClick={() => {
                    run("more", async () => {
                      await sessions.fetchNextPage({ throwOnError: true });
                    });
                  }}
                >
                  {busyId === "more" ? "Loading…" : "Load more sessions"}
                </Button>
              </div>
            )}
            {feedback("more")}
          </div>
        </>
      )}
      <div className="grid gap-3">
        {confirmAll ? (
          <>
            <p className="text-sm leading-relaxed">
              Sign out every native session, including this installation? You will need to sign in
              again to use your account. Your local files and execution nodes remain available.
            </p>
            <div className="flex flex-wrap gap-2">
              <Button
                variant="destructive"
                disabled={busy}
                onClick={() => {
                  revokeAll();
                }}
              >
                {busyId === "all" ? "Signing out…" : "Confirm sign out everywhere"}
              </Button>
              <Button
                ref={cancelConfirmation}
                variant="ghost"
                disabled={busy}
                onClick={() => {
                  setConfirmAll(false);
                }}
              >
                Cancel
              </Button>
            </div>
            {feedback("all")}
          </>
        ) : (
          <div>
            <Button
              variant="outline"
              disabled={busy}
              onClick={() => {
                setConfirmAll(true);
              }}
            >
              Sign out all native sessions
            </Button>
          </div>
        )}
      </div>
    </section>
  );
}
