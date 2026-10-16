import { Tooltip, Button, Dropdown, TooltipContent } from "@swarmie/ui-web";
import { LogOut, Settings, UserRound, ShieldCheck } from "lucide-react";
// Copyright 2026 Syntropika
// SPDX-License-Identifier: Apache-2.0
import { useState } from "react";

import { useAccountService, useAccount } from "~/src/account-state";
import { startAction, errorMessage, errorReporter } from "~/src/action-boundary";
import { hasText, nonEmptyText } from "~/src/value-presence";

export function AccountMenu({
  onOpen,
  onNotice,
}: {
  onOpen: (section: "general" | "account" | "security") => void;
  onNotice: (message: string) => void;
}) {
  const service = useAccountService();
  const { account, error: observationError } = useAccount();
  const [busy, setBusy] = useState(false);
  const [failedImage, setFailedImage] = useState<string>();
  const profile = account?.status === "signedIn" ? account.profile : null;
  const label =
    nonEmptyText(profile?.displayName) ??
    nonEmptyText(profile?.email) ??
    (() => {
      if (observationError) {
        return "Account unavailable";
      }
      if (account?.status === "signingIn") {
        return "Sign-in in progress";
      }
      if (account?.status === "storageUnavailable") {
        return "Unlock account storage";
      }
      if (service.available() && !account) {
        return "Checking account";
      }
      return "Sign in";
    })();
  const avatarUrl = profile?.avatarUrl;
  const logout = async () => {
    if (!profile || busy) {
      return;
    }
    const initiatingAccountId = profile.id;
    const notice = (message: string) => {
      const current = service.getSnapshot().account;
      if (
        current?.status === "signingIn" ||
        (current?.profile && current.profile.id !== initiatingAccountId)
      ) {
        return;
      }
      onNotice(message);
    };
    setBusy(true);
    try {
      const result = await service.request("account/logout", { expectedAccountId: profile.id });
      await service.reconcile();
      if (!result.remoteRevocationConfirmed) {
        notice("Signed out locally. Remote revocation could not be confirmed.");
      }
    } catch (error) {
      notice(errorMessage(error));
    } finally {
      setBusy(false);
    }
  };
  return (
    <Dropdown>
      <Tooltip>
        <Button
          variant="tertiary"
          size="md"
          isIconOnly
          aria-label={`Account: ${label}`}
          aria-busy={busy}
        >
          {hasText(avatarUrl) && failedImage !== avatarUrl ? (
            <img
              className="size-6 rounded-full object-cover"
              src={avatarUrl}
              alt=""
              referrerPolicy="no-referrer"
              onError={() => {
                setFailedImage(avatarUrl);
              }}
            />
          ) : (
            <UserRound size={18} />
          )}
        </Button>
        <TooltipContent placement="right">{label}</TooltipContent>
      </Tooltip>
      <Dropdown.Popover placement="top end">
        <div className="max-w-64 px-2 py-2 text-sm">
          <p className="font-medium break-words">{label}</p>
          {hasText(profile?.displayName) && (
            <p className="text-xs break-all text-muted">{profile.email}</p>
          )}
          {observationError && (
            <p className="mt-1 text-xs text-danger">
              Account updates unavailable. Open your profile to try again.
            </p>
          )}
        </div>
        <Dropdown.Menu aria-label="Account menu">
          <Dropdown.Item
            id="profile"
            textValue={profile ? "Profile" : "Sign in"}
            onAction={() => {
              onOpen("account");
            }}
          >
            <UserRound size={14} />
            {(() => {
              if (profile) {
                return "Profile";
              }
              if (account?.status === "signingIn") {
                return "Continue sign-in";
              }
              if (account?.status === "storageUnavailable") {
                return "Account storage";
              }
              return "Sign in";
            })()}
          </Dropdown.Item>
          <Dropdown.Item
            id="settings"
            textValue="Settings"
            onAction={() => {
              onOpen("general");
            }}
          >
            <Settings size={14} />
            Settings
          </Dropdown.Item>
          {profile && (
            <Dropdown.Item
              id="security"
              textValue="Account security"
              onAction={() => {
                onOpen("security");
              }}
            >
              <ShieldCheck size={14} />
              Account security
            </Dropdown.Item>
          )}
          {profile && (
            <Dropdown.Item
              id="logout"
              textValue="Sign out"
              isDisabled={busy}
              onAction={() => {
                startAction(logout(), errorReporter(onNotice));
              }}
            >
              <LogOut size={14} />
              Sign out
            </Dropdown.Item>
          )}
        </Dropdown.Menu>
      </Dropdown.Popover>
    </Dropdown>
  );
}
