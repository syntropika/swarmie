// Copyright 2026 Syntropika
// SPDX-License-Identifier: Apache-2.0
import { useState } from "react";
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
    </> : <>
      <p className="text-sm text-muted">{account ? "Sign in to review this installation’s account session." : "Checking your account…"}</p>
      <div><Button variant="outline" onClick={onProfile}>Open account</Button></div>
    </>}
  </section>;
}
