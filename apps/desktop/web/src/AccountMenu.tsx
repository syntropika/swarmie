// Copyright 2026 Syntropika
// SPDX-License-Identifier: Apache-2.0
import { useState } from "react";
import { LogOut, Settings, UserRound, ShieldCheck } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Dropdown } from "@/components/ui/dropdown";
import { TooltipContent } from "@/components/ui/tooltip";
import { accountRequest } from "./account";
import { accountStore, useAccount } from "./accountState";
import { nativeClient } from "./clientTransport";

export function AccountMenu({ onOpen, onNotice }: {
  onOpen: (section: "general" | "account" | "security") => void;
  onNotice: (message: string) => void;
}) {
  const { account, error } = useAccount();
  const [busy, setBusy] = useState(false);
  const [failedImage, setFailedImage] = useState<string>();
  const profile = account?.status === "signedIn" ? account.profile : null;
  const label = profile?.displayName || profile?.email || (error ? "Account unavailable" : account?.status === "signingIn" ? "Sign-in in progress" : account?.status === "storageUnavailable" ? "Unlock account storage" : nativeClient() && !account ? "Checking account" : "Sign in");
  const logout = async () => {
    if (!profile || busy) return;
    setBusy(true);
    try {
      const result = await accountRequest("account/logout", { expectedAccountId: profile.id });
      await accountStore.reconcile();
      if (!result.remoteRevocationConfirmed) onNotice("Signed out locally. Remote revocation could not be confirmed.");
    } catch (error) { onNotice(error instanceof Error ? error.message : String(error)); }
    finally { setBusy(false); }
  };
  return <Dropdown triggerTooltip={<TooltipContent placement="right">{label}</TooltipContent>}>
    <Button variant="ghost" size="icon" className="icon-button" aria-label={`Account: ${label}`} aria-busy={busy}>
      {profile?.avatarUrl && failedImage !== profile.avatarUrl
        ? <img className="size-6 rounded-full object-cover" src={profile.avatarUrl} alt="" referrerPolicy="no-referrer" onError={() => setFailedImage(profile.avatarUrl!)} />
        : <UserRound size={18} />}
    </Button>
    <Dropdown.Popover className="workspace-menu" placement="top end">
      <Dropdown.Menu aria-label="Account menu">
        <div className="max-w-64 px-2 py-2 text-sm">
          <p className="break-words font-medium">{label}</p>
          {profile?.displayName && <p className="break-all text-xs text-muted">{profile.email}</p>}
          {error && <p className="mt-1 text-xs text-danger">Account updates unavailable. Open your profile to try again.</p>}
        </div>
        <Dropdown.Item id="profile" textValue={profile ? "Profile" : "Sign in"} onAction={() => onOpen("account")}><UserRound size={14} />{profile ? "Profile" : account?.status === "signingIn" ? "Continue sign-in" : account?.status === "storageUnavailable" ? "Account storage" : "Sign in"}</Dropdown.Item>
        <Dropdown.Item id="settings" textValue="Settings" onAction={() => onOpen("general")}><Settings size={14} />Settings</Dropdown.Item>
        {profile && <Dropdown.Item id="security" textValue="Account security" onAction={() => onOpen("security")}><ShieldCheck size={14} />Account security</Dropdown.Item>}
        {profile && <Dropdown.Item id="logout" textValue="Sign out" isDisabled={busy} onAction={() => void logout()}><LogOut size={14} />Sign out</Dropdown.Item>}
      </Dropdown.Menu>
    </Dropdown.Popover>
  </Dropdown>;
}
