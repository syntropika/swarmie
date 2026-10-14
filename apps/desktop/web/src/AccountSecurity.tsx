// Copyright 2026 Syntropika
// SPDX-License-Identifier: Apache-2.0
import { useEffect, useRef, useState } from "react";
import type { AccountSession, AccountSessionPage } from "@swarmie/contracts";
import { Button } from "@/components/ui/button";
import { useAccount, accountStore } from "./accountState";
import { accountRequest } from "./account";

export function AccountSecurity({ onProfile }: { onProfile: () => void }) {
  const { account, error: observationError } = useAccount();
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");
  const profile = account?.status === "signedIn" ? account.profile : null;
  const logout = async () => {
    if (!profile) return;
    setBusy(true); setError(""); setMessage("");
    try {
      const result = await accountRequest("account/logout", { expectedAccountId: profile.id });
      await accountStore.reconcile();
      setMessage(result.remoteRevocationConfirmed ? "This installation is signed out." : "Signed out locally. Remote revocation could not be confirmed.");
    } catch (error) { setError(error instanceof Error ? error.message : String(error)); }
    finally { setBusy(false); }
  };
  return <section className="grid gap-5" aria-label="Account security">
    {(error || observationError) && <p role="alert" className="text-sm text-danger">{error || observationError}</p>}
    {message && <p role="status" className="text-sm text-muted">{message}</p>}
    {profile ? <>
      <dl className="grid gap-2 text-sm"><dt className="text-muted">Signed in as</dt><dd className="break-all">{profile.email}</dd><dt className="mt-2 text-muted">Login storage</dt><dd>{account?.persistence === "session" ? "For this owner session only" : "Protected by your system credential vault"}</dd></dl>
      <p className="text-sm leading-relaxed text-muted">Closing Swarmie keeps this login available. Signing out removes it from this installation and asks the account service to revoke its access. Your browser login has its own session.</p>
      <div><Button variant="outline" disabled={busy} onClick={() => void logout()}>{busy ? "Signing out…" : "Sign out of this installation"}</Button></div>
      <NativeSessions key={profile.id} accountId={profile.id} onMessage={setMessage} onError={setError} />
    </> : <>
      <p className="text-sm text-muted">{account ? "Sign in to review this installation’s account session." : "Checking your account…"}</p>
      <div><Button variant="outline" onClick={onProfile}>Open account</Button></div>
    </>}
  </section>;
}

function date(value: string) {
  const parsed = new Date(value);
  return Number.isNaN(parsed.getTime()) ? "Unavailable" : parsed.toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
}
function SessionDetails({ session }: { session: AccountSession }) {
  return <dl className="grid gap-1 text-sm text-muted">
    <div><dt className="inline">Signed in </dt><dd className="inline"><time dateTime={session.createdAt}>{date(session.createdAt)}</time></dd></div>
    <div><dt className="inline">Last active </dt><dd className="inline"><time dateTime={session.lastUsedAt}>{date(session.lastUsedAt)}</time></dd></div>
    <div><dt className="inline">Expires </dt><dd className="inline"><time dateTime={session.expiresAt}>{date(session.expiresAt)}</time></dd></div>
  </dl>;
}
function SessionFailure({ message, onRefresh, disabled }: { message: string; onRefresh: () => void; disabled: boolean }) {
  const recovery = useRef<HTMLButtonElement>(null);
  useEffect(() => { if (!disabled) recovery.current?.focus(); }, [message, disabled]);
  return <div className="grid gap-2"><p role="alert" className="text-sm text-danger">{message} Refresh the session state before trying again.</p><div><Button ref={recovery} variant="ghost" disabled={disabled} onClick={onRefresh}>Refresh session state</Button></div></div>;
}
function NativeSessions({ accountId, onMessage, onError }: { accountId: string; onMessage: (value: string) => void; onError: (value: string) => void }) {
  const [page, setPage] = useState<AccountSessionPage | null>(null);
  const [busy, setBusy] = useState("loading");
  const [confirmAll, setConfirmAll] = useState(false);
  const [failure, setFailure] = useState<{ id: string; message: string } | null>(null);
  const cancelConfirmation = useRef<HTMLButtonElement>(null);
  useEffect(() => { if (confirmAll) cancelConfirmation.current?.focus(); }, [confirmAll]);
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    accountRequest("account/sessions/read", { expectedAccountId: accountId }).then((value) => {
      if (alive.current) setPage(value);
    }).catch((error) => {
      if (alive.current) setFailure({ id: "read", message: error instanceof Error ? error.message : String(error) });
      void accountStore.reconcile().catch(() => {});
    }).finally(() => { if (alive.current) setBusy(""); });
    return () => { alive.current = false; };
  }, [accountId, onError]);
  const load = async (after?: string) => {
    const next = await accountRequest("account/sessions/read", { expectedAccountId: accountId, ...(after ? { after } : {}) });
    if (alive.current) setPage((previous) => after && previous ? { ...next, others: [...previous.others, ...next.others].filter((session, index, all) => all.findIndex((item) => item.id === session.id) === index) } : next);
  };
  const run = async (id: string, action: () => Promise<unknown>) => {
    setBusy(id); setFailure(null); onError(""); onMessage("");
    try { await action(); }
    catch (error) {
      if (alive.current) setFailure({ id, message: error instanceof Error ? error.message : String(error) });
      await accountStore.reconcile().catch(() => {});
    } finally { if (alive.current) setBusy(""); }
  };
  const revoke = (session: AccountSession) => run(session.id, async () => {
    await accountRequest("account/sessions/revoke", { expectedAccountId: accountId, sessionId: session.id });
    if (alive.current) onMessage("The selected native session is signed out.");
    await load();
  });
  const revokeAll = () => run("all", async () => {
    await accountRequest("account/sessions/revoke-all", { expectedAccountId: accountId });
    if (alive.current) onMessage("All native sessions are signed out. Your browser session remains available.");
    await accountStore.reconcile();
  });
  const feedback = (id: string) => failure?.id === id ? <SessionFailure message={failure.message} disabled={Boolean(busy)} onRefresh={() => void run("refresh", () => load())} /> : null;
  return <section className="grid min-w-0 gap-4 border-t border-border pt-6" aria-label="Native sessions" aria-busy={Boolean(busy)}>
    <div className="flex flex-wrap items-center justify-between gap-2"><h3 className="text-sm font-semibold">Native sessions</h3><Button variant="ghost" disabled={Boolean(busy)} onClick={() => void run("refresh", () => load())}>{busy === "refresh" ? "Refreshing…" : "Refresh sessions"}</Button></div>
    <p className="text-sm leading-relaxed text-muted">These sessions connect Swarmie installations to your account. Activity times can lag by five minutes. Your browser session is managed separately.</p>
    {feedback("read")}{feedback("refresh")}
    {busy === "loading" && <p role="status" className="text-sm text-muted">Loading sessions…</p>}
    {page && <>
      <div className="grid gap-2"><h4 className="text-sm font-medium">This installation</h4><SessionDetails session={page.current} /></div>
      <div className="grid gap-2"><h4 className="text-sm font-medium">Other native sessions</h4>
        {page.others.length ? <ul className="grid divide-y divide-border">{page.others.map((session) => <li key={session.id} className="grid min-w-0 gap-3 py-4">
          <div className="flex flex-wrap items-center justify-between gap-2"><span className="text-sm font-medium">Session {session.id.slice(0, 8)}</span><Button variant="outline" aria-label={`Sign out session ${session.id.slice(0, 8)}`} disabled={Boolean(busy)} onClick={() => void revoke(session)}>{busy === session.id ? "Signing out…" : "Sign out"}</Button></div>
          {feedback(session.id)}
          <SessionDetails session={session} />
          <p className="break-all text-xs text-muted">Installation {session.installationId}</p>
        </li>)}</ul> : <p className="text-sm text-muted">No other active native sessions.</p>}
        {page.nextCursor && <div><Button variant="outline" disabled={Boolean(busy)} onClick={() => void run("more", () => load(page.nextCursor!))}>{busy === "more" ? "Loading…" : "Load more sessions"}</Button></div>}
        {feedback("more")}
      </div>
    </>}
    <div className="grid gap-3">
      {confirmAll ? <>
        <p className="text-sm leading-relaxed">Sign out every native session, including this installation? You will need to sign in again to use your account. Your local files and execution nodes remain available.</p>
        <div className="flex flex-wrap gap-2"><Button variant="outline" className="text-danger" disabled={Boolean(busy)} onClick={() => void revokeAll()}>{busy === "all" ? "Signing out…" : "Confirm sign out everywhere"}</Button><Button ref={cancelConfirmation} variant="ghost" disabled={Boolean(busy)} onClick={() => setConfirmAll(false)}>Cancel</Button></div>
        {feedback("all")}
      </> : <div><Button variant="outline" disabled={Boolean(busy)} onClick={() => setConfirmAll(true)}>Sign out all native sessions</Button></div>}
    </div>
  </section>;
}
