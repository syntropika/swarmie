// Copyright 2026 Syntropika
// SPDX-License-Identifier: Apache-2.0
import type { AccountMethods, AccountProfile, AccountSnapshot } from "@swarmie/contracts";
import { useId, useState } from "react";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";

import { accountReceiptId } from "./account";
import { useAccountAction } from "./account-action";
import { useAccountService } from "./account-state";
import { hasText, nonEmptyText } from "./value-presence";

type ProfileReceipt = AccountMethods["account/profile/update"]["params"];
type AvatarReceipt = AccountMethods["account/profile/avatar/update"]["params"];
type Draft = { name: string; revision?: number; receipt?: ProfileReceipt };

async function imageBytes(file: File) {
  if (file.size > 1024 * 1024) {
    throw new Error("Choose a PNG, JPEG or WebP image under 1 MiB.");
  }
  const bytes = new Uint8Array(await file.arrayBuffer());
  let binary = "";
  for (const byte of bytes) {
    binary += String.fromCodePoint(byte);
  }
  return btoa(binary);
}

export function AccountProfileEditor({
  profile,
  persistence,
}: {
  profile: AccountProfile;
  persistence: AccountSnapshot["persistence"];
}) {
  const service = useAccountService();
  const [notice, setNotice] = useState("");
  const { run, busy } = useAccountAction(setNotice);
  const [draft, setDraft] = useState<Draft>({ name: profile.displayName ?? "" });
  const [avatarReceipt, setAvatarReceipt] = useState<AvatarReceipt>();
  const [observedProfile, setObservedProfile] = useState(profile);
  // Preserve edits and their CAS revision while observations refresh the untouched name.
  if (observedProfile !== profile) {
    setObservedProfile(profile);
    if (draft.revision === undefined) {
      setDraft({ name: profile.displayName ?? "" });
    }
  }
  const nameId = useId();
  const imageId = useId();
  const updateImage = async (body: AvatarReceipt) => {
    setAvatarReceipt(body);
    await service.request("account/profile/avatar/update", body);
    await service.reconcile();
    setAvatarReceipt(undefined);
  };
  const chooseImage = (file: File) => {
    // Capture the initiating principal and revision before reading the file.
    const { id: accountId, revision: expectedRevision } = profile;
    run(async () => {
      const body = {
        accountId,
        expectedRevision,
        imageBase64: await imageBytes(file),
        requestId: accountReceiptId(),
      };
      await updateImage(body);
    });
  };
  const saveProfile = () => {
    const body = draft.receipt ?? {
      accountId: profile.id,
      requestId: accountReceiptId(),
      expectedRevision: draft.revision ?? profile.revision,
      displayName: nonEmptyText(draft.name.trim()) ?? null,
    };
    setDraft({ ...draft, receipt: body });
    run(async () => {
      await service.request("account/profile/update", body);
      const saved = await service.reconcile();
      if (saved.profile?.id === body.accountId) {
        setDraft({ name: saved.profile.displayName ?? "" });
        setNotice("Profile saved.");
      }
    });
  };
  const refresh = () => {
    run(async () => {
      const current = await service.refresh();
      if (current.profile?.id === profile.id) {
        setDraft((previous) => ({
          name: previous.name,
          revision: previous.revision === undefined ? undefined : current.profile?.revision,
        }));
        setAvatarReceipt(undefined);
      }
      setNotice("Profile refreshed.");
    });
  };
  return (
    <div className="grid gap-5">
      {notice && <output className="text-muted text-sm">{notice}</output>}
      <div className="flex flex-wrap items-center gap-4">
        {hasText(profile.avatarUrl) && (
          <img
            className="size-16 rounded-full object-cover"
            src={profile.avatarUrl}
            alt="Your profile"
            referrerPolicy="no-referrer"
          />
        )}
        <div className="grid gap-2">
          <label htmlFor={imageId} className="text-sm">
            Profile image
          </label>
          <Input
            id={imageId}
            type="file"
            accept="image/png,image/jpeg,image/webp"
            disabled={busy || avatarReceipt !== undefined}
            onChange={(event) => {
              const file = event.target.files?.[0];
              event.target.value = "";
              if (file) {
                chooseImage(file);
              }
            }}
          />
          <span className="text-muted text-xs">
            PNG, JPEG or WebP. Up to 1 MiB and 2048 pixels per side.
          </span>
        </div>
        {hasText(profile.avatarUrl) && (
          <Button
            variant="outline"
            disabled={busy || avatarReceipt !== undefined}
            onClick={() => {
              run(async () => {
                await updateImage({
                  accountId: profile.id,
                  imageBase64: null,
                  expectedRevision: profile.revision,
                  requestId: accountReceiptId(),
                });
              });
            }}
          >
            Remove image
          </Button>
        )}
      </div>
      {avatarReceipt && (
        <div>
          <Button
            variant="outline"
            disabled={busy}
            onClick={() => {
              run(async () => {
                await updateImage(avatarReceipt);
              });
            }}
          >
            Retry image change
          </Button>
        </div>
      )}
      <dl className="grid gap-2 text-sm">
        <dt className="text-muted">Email</dt>
        <dd className="break-all">
          {profile.email}
          {profile.emailVerified ? " · Verified" : " · Unverified"}
        </dd>
      </dl>
      <form
        className="grid gap-3"
        onSubmit={(event) => {
          event.preventDefault();
          saveProfile();
        }}
      >
        <label htmlFor={nameId} className="text-sm">
          Display name
        </label>
        <Input
          id={nameId}
          autoComplete="name"
          maxLength={120}
          value={draft.name}
          disabled={busy || avatarReceipt !== undefined}
          onChange={(event) => {
            setDraft({ name: event.target.value, revision: draft.revision ?? profile.revision });
          }}
        />
        <div className="flex flex-wrap gap-2">
          <Button
            type="submit"
            disabled={
              busy || avatarReceipt !== undefined || draft.name === (profile.displayName ?? "")
            }
          >
            Save profile
          </Button>
          <Button type="button" variant="outline" disabled={busy} onClick={refresh}>
            Refresh profile
          </Button>
        </div>
      </form>
      {persistence === "session" && (
        <p className="text-muted text-sm">This login lasts until the account owner stops.</p>
      )}
      <div>
        <Button
          variant="outline"
          disabled={busy}
          onClick={() => {
            run(async () => {
              const result = await service.request("account/logout", {
                expectedAccountId: profile.id,
              });
              await service.reconcile();
              if (!result.remoteRevocationConfirmed) {
                setNotice("Signed out locally. Remote revocation could not be confirmed.");
              }
            });
          }}
        >
          Sign out
        </Button>
      </div>
    </div>
  );
}
