// Copyright 2026 Syntropika
// SPDX-License-Identifier: Apache-2.0
import { useEffect, useRef, useState } from "react";
import type { AccountSnapshot, AccountMethods } from "@swarmie/contracts";
import { accountRequest } from "./account";
import { nativeClient } from "./clientTransport";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";

export function AccountSettings() {
  const [account, setAccount] = useState<AccountSnapshot>();
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [busy, setBusy] = useState(false);
  const [name, setName] = useState("");
  const [origin, setOrigin] = useState("");
  const [sessionOnly, setSessionOnly] = useState(false);
  const [loginUrl, setLoginUrl] = useState("");
  const [receipt, setReceipt] = useState<{ accountId: string; requestId: string; expectedRevision: number; displayName: string | null }>();
  const [avatarReceipt, setAvatarReceipt] = useState<AccountMethods["account/profile/avatar/update"]["params"]>();
  const active = useRef(true);
  const apply = (value: AccountSnapshot) => { if (active.current) { setAccount(value); if (value.status !== "signingIn") setLoginUrl(""); } };
  const refresh = async () => { const value = await accountRequest("account/read", {}); apply(value); return value; };
  const run = async (action: () => Promise<unknown>) => {
    setBusy(true); setError(""); setNotice("");
    try { await action(); } catch (error) { if (active.current) setError(error instanceof Error ? error.message : String(error)); }
    finally { if (active.current) setBusy(false); }
  };
  useEffect(() => {
    active.current = true;
    if (nativeClient()) void run(refresh);
    return () => { active.current = false; }; // Closing this view does not cancel native sign-in.
  }, []);
  useEffect(() => {
    if (account?.status !== "signingIn") return;
    let pending = false;
    const timer = setInterval(() => { if (pending) return; pending = true; void refresh().catch((error) => { if (active.current) setError(String(error)); }).finally(() => { pending = false; }); }, 2000);
    return () => clearInterval(timer);
  }, [account?.status]);
  useEffect(() => { setName(account?.profile?.displayName ?? ""); setReceipt(undefined); setAvatarReceipt(undefined); }, [account?.profile?.revision, account?.profile?.id]);
  if (!nativeClient()) return <p className="text-sm text-muted">Open the desktop app to manage your account. You can continue using local projects without signing in.</p>;
  return <section className="grid gap-5" aria-label="Swarmie account">
    <p className="text-sm leading-relaxed text-muted">Sign in to connect your account. Your projects, files and nodes stay private until you choose to share them.</p>
    {error && <p role="alert" className="text-sm text-danger">{error}</p>}
    {notice && <p role="status" className="text-sm text-muted">{notice}</p>}
    {account?.status === "storageUnavailable" && <p role="status" className="text-sm text-muted">Your system vault is locked or unavailable. Unlock it and check again, or explicitly sign in for this owner session only.</p>}
    {account?.status === "storageUnavailable" && <div><Button variant="outline" disabled={busy} onClick={() => void run(async () => { const result = await accountRequest("account/logout", {}); apply(result.account); if (!result.remoteRevocationConfirmed) setNotice("Saved login forgotten locally. Remote revocation could not be confirmed."); })}>Forget saved login</Button></div>}
    {!account && <p role="status" className="text-sm text-muted">Checking your account…</p>}
    {account?.configured === false && <form className="grid gap-3" onSubmit={(event) => { event.preventDefault(); void run(async () => apply(await accountRequest("account/configure", { origin }))); }}>
      <label className="grid gap-2 text-sm">Account service<Input type="url" required placeholder="https://accounts.example.com" value={origin} onChange={(event) => setOrigin(event.target.value)} disabled={busy} /></label>
      <div><Button type="submit" disabled={busy || !origin}>Set account service</Button></div>
    </form>}
    {(account?.status === "signedOut" || account?.status === "storageUnavailable") && account.configured !== false && <div className="grid gap-4">
      <Switch label="Sign in for this owner session only" checked={sessionOnly} disabled={busy} onChange={setSessionOnly} />
      {sessionOnly && <p className="text-sm text-muted">You will need to sign in again after the account owner stops.</p>}
      <div><Button disabled={busy} onClick={() => void run(async () => { const login = await accountRequest("account/login/start", { sessionOnly }); setLoginUrl(login.authorizationUrl); await refresh(); })}>{busy ? "Opening sign-in…" : "Sign in"}</Button></div>
    </div>}
    {account?.status === "signingIn" && <div className="grid gap-3">
      <p role="status" className="text-sm">Complete sign-in in your browser. You can close this window and return later.</p>
      {loginUrl && <a className="text-sm underline underline-offset-4" href={loginUrl} target="_blank" rel="noreferrer">Open sign-in in browser</a>}
      <div><Button variant="outline" disabled={busy} onClick={() => void run(async () => apply(await accountRequest("account/login/cancel", { loginId: account.loginId! })))}>Cancel sign-in</Button></div>
    </div>}
    {account?.status === "signedIn" && account.profile && <div className="grid gap-5">
      <div className="flex flex-wrap items-center gap-4">
        {account.profile.avatarUrl && <img className="size-16 rounded-full object-cover" src={account.profile.avatarUrl} alt="Your profile" referrerPolicy="no-referrer" />}
        <label className="grid gap-2 text-sm">Profile image<Input type="file" accept="image/png,image/jpeg,image/webp" disabled={busy || !!avatarReceipt} onChange={(event) => {
          const file = event.target.files?.[0]; event.target.value = ""; if (!file) return;
          void run(async () => {
            if (file.size > 1024 * 1024) throw new Error("Choose a PNG, JPEG or WebP image under 1 MiB.");
            const bytes = new Uint8Array(await file.arrayBuffer()); let binary = ""; for (const byte of bytes) binary += String.fromCharCode(byte);
            const body = { accountId: account.profile!.id, imageBase64: btoa(binary), expectedRevision: account.profile!.revision, requestId: crypto.randomUUID() }; setAvatarReceipt(body);
            apply(await accountRequest("account/profile/avatar/update", body)); setAvatarReceipt(undefined);
          });
        }} /><span className="text-xs text-muted">PNG, JPEG or WebP. Up to 1 MiB and 2048 pixels per side.</span></label>
        {account.profile.avatarUrl && <Button variant="outline" disabled={busy || !!avatarReceipt} onClick={() => void run(async () => { const body = { accountId: account.profile!.id, imageBase64: null, expectedRevision: account.profile!.revision, requestId: crypto.randomUUID() }; setAvatarReceipt(body); apply(await accountRequest("account/profile/avatar/update", body)); setAvatarReceipt(undefined); })}>Remove image</Button>}
      </div>
      {avatarReceipt && <div><Button variant="outline" disabled={busy} onClick={() => void run(async () => { apply(await accountRequest("account/profile/avatar/update", avatarReceipt)); setAvatarReceipt(undefined); })}>Retry image change</Button></div>}
      <dl className="grid gap-2 text-sm"><dt className="text-muted">Email</dt><dd className="break-all">{account.profile.email}{account.profile.emailVerified ? " · Verified" : " · Unverified"}</dd></dl>
      <form className="grid gap-3" onSubmit={(event) => { event.preventDefault(); const body = receipt ?? { accountId: account.profile!.id, requestId: crypto.randomUUID(), expectedRevision: account.profile!.revision, displayName: name.trim() || null }; setReceipt(body); void run(async () => { apply(await accountRequest("account/profile/update", body)); setReceipt(undefined); setNotice("Profile saved."); }); }}>
        <label className="grid gap-2 text-sm">Display name<Input autoComplete="name" maxLength={120} value={name} disabled={busy || !!avatarReceipt} onChange={(event) => { setName(event.target.value); setReceipt(undefined); }} /></label>
        <div className="flex flex-wrap gap-2"><Button type="submit" disabled={busy || !!avatarReceipt || name === (account.profile.displayName ?? "")}>Save profile</Button><Button type="button" variant="outline" disabled={busy} onClick={() => void run(async () => { apply(await refresh()); setNotice("Profile refreshed."); })}>Refresh profile</Button></div>
      </form>
      {account.persistence === "session" && <p className="text-sm text-muted">This login lasts until the account owner stops.</p>}
      <div><Button variant="outline" disabled={busy} onClick={() => void run(async () => { const result = await accountRequest("account/logout", { expectedAccountId: account.profile!.id }); apply(result.account); if (!result.remoteRevocationConfirmed) setNotice("Signed out locally. Remote revocation could not be confirmed."); })}>Sign out</Button></div>
    </div>}
    {(error || account?.status === "storageUnavailable") && <div><Button variant="outline" disabled={busy} onClick={() => void run(refresh)}>Check account again</Button></div>}
  </section>;
}
