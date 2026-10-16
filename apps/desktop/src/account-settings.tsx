// Copyright 2026 Syntropika
// SPDX-License-Identifier: Apache-2.0
import type { AccountSnapshot } from "@swarmie/contracts";
import { Button, Input, Switch } from "@swarmie/ui-web";
import { useId, useState } from "react";

import { useAccountAction } from "~/src/account-action";
import { AccountProfileEditor } from "~/src/account-profile-editor";
import { useAccountService, useAccount } from "~/src/account-state";
import { hasText } from "~/src/value-presence";

function AccountSignIn({ account }: { account: AccountSnapshot }) {
  const service = useAccountService();
  const [notice, setNotice] = useState("");
  const [origin, setOrigin] = useState("");
  const [sessionOnly, setSessionOnly] = useState(false);
  const { run, busy } = useAccountAction(setNotice);
  const originId = useId();
  const { loginId } = account;
  return (
    <div className="grid gap-4">
      {notice && <output className="text-sm text-muted">{notice}</output>}
      {account.status === "storageUnavailable" && (
        <>
          <output className="text-sm text-muted">
            Your system vault is locked or unavailable. Unlock it and check again, or explicitly
            sign in for this owner session only.
          </output>
          <div>
            <Button
              variant="outline"
              disabled={busy}
              onClick={() => {
                run(async () => {
                  const result = await service.request("account/logout", {});
                  await service.reconcile();
                  if (!result.remoteRevocationConfirmed) {
                    setNotice(
                      "Saved login forgotten locally. Remote revocation could not be confirmed.",
                    );
                  }
                });
              }}
            >
              Forget saved login
            </Button>
          </div>
        </>
      )}
      {account.configured === false && (
        <form
          className="grid gap-3"
          onSubmit={(event) => {
            event.preventDefault();
            run(async () => {
              await service.request("account/configure", { origin });
              await service.reconcile();
            });
          }}
        >
          <label htmlFor={originId} className="text-sm">
            Account service
          </label>
          <Input
            id={originId}
            type="url"
            required
            placeholder="https://accounts.example.com"
            value={origin}
            onChange={(event) => {
              setOrigin(event.target.value);
            }}
            disabled={busy}
          />
          <div>
            <Button type="submit" disabled={busy || !origin}>
              Set account service
            </Button>
          </div>
        </form>
      )}
      {(account.status === "signedOut" || account.status === "storageUnavailable") &&
        account.configured !== false && (
          <>
            <Switch
              isSelected={sessionOnly}
              isDisabled={busy}
              onChange={setSessionOnly}
              aria-label="Sign in for this owner session only"
            >
              <Switch.Content>
                <Switch.Control>
                  <Switch.Thumb />
                </Switch.Control>
                Sign in for this owner session only
              </Switch.Content>
            </Switch>
            {sessionOnly && (
              <p className="text-sm text-muted">
                You will need to sign in again after the account owner stops.
              </p>
            )}
            <div>
              <Button
                disabled={busy}
                onClick={() => {
                  run(async () => {
                    await service.request("account/login/start", { sessionOnly });
                    await service.reconcile();
                  });
                }}
              >
                {busy ? "Opening sign-in…" : "Sign in"}
              </Button>
            </div>
          </>
        )}
      {account.status === "signingIn" && (
        <>
          <output className="text-sm">
            Complete sign-in in your browser. You can close this window and return later.
          </output>
          {hasText(account.authorizationUrl) && (
            <a
              className="text-sm underline underline-offset-4"
              href={account.authorizationUrl}
              target="_blank"
              rel="noreferrer"
            >
              Open sign-in in browser
            </a>
          )}
          {loginId !== null && (
            <div>
              <Button
                variant="outline"
                disabled={busy}
                onClick={() => {
                  run(async () => {
                    await service.request("account/login/cancel", { loginId });
                    await service.reconcile();
                  });
                }}
              >
                Cancel sign-in
              </Button>
            </div>
          )}
        </>
      )}
    </div>
  );
}

export function AccountSettings() {
  const service = useAccountService();
  const { account, error: observationError } = useAccount();
  const [notice, setNotice] = useState("");
  const { run, busy } = useAccountAction(setNotice);
  if (!service.available()) {
    return (
      <p className="text-sm text-muted">
        Open the desktop app to manage your account. You can continue using local projects without
        signing in.
      </p>
    );
  }
  return (
    <section className="grid gap-5" aria-label="Swarmie account">
      <p className="text-sm leading-relaxed text-muted">
        Sign in to connect your account. Your projects, files and nodes stay private until you
        choose to share them.
      </p>
      {observationError && (
        <p role="alert" className="text-sm text-danger">
          {observationError}
        </p>
      )}
      {notice && <output className="text-sm text-muted">{notice}</output>}
      {!account && <output className="text-sm text-muted">Checking your account…</output>}
      {account && <AccountSignIn key={account.profile?.id ?? "signed-out"} account={account} />}
      {account?.status === "signedIn" && account.profile && (
        <AccountProfileEditor
          key={account.profile.id}
          profile={account.profile}
          persistence={account.persistence}
        />
      )}
      {(hasText(observationError) || account?.status === "storageUnavailable") && (
        <div>
          <Button
            variant="outline"
            disabled={busy}
            onClick={() => {
              run(async () => {
                await service.refresh();
              });
            }}
          >
            Check account again
          </Button>
        </div>
      )}
    </section>
  );
}
