// @vitest-environment happy-dom
// Copyright 2026 Syntropika
// SPDX-License-Identifier: Apache-2.0
import type { AccountMethods } from "@swarmie/contracts";
import { act, screen, cleanup, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, it, vi } from "vitest";

import { AccountMenu } from "./account-menu";
import {
  AccountFixture,
  accountSnapshot,
  accountProfile,
  renderAccount,
} from "./account-test-fixture";
import { deferred } from "./test-fixtures";

afterEach(cleanup);
it("keeps settings available without login and directs sign-in to the account page", async () => {
  const fixture = new AccountFixture();
  const onOpen = vi.fn<(section: "general" | "account" | "security") => void>();
  renderAccount(
    fixture,
    <AccountMenu onOpen={onOpen} onNotice={vi.fn<(message: string) => void>()} />,
  );
  const user = userEvent.setup();
  await user.click(await screen.findByRole("button", { name: "Account: Sign in" }));
  await user.click(screen.getByRole("menuitem", { name: "Settings" }));
  expect(onOpen).toHaveBeenCalledWith("general");
  await user.click(screen.getByRole("button", { name: "Account: Sign in" }));
  await user.click(screen.getByRole("menuitem", { name: "Sign in" }));
  expect(onOpen).toHaveBeenCalledWith("account");
});
it("shows identity, opens account security and reports uncertain remote revocation", async () => {
  const fixture = new AccountFixture(
    accountSnapshot({
      status: "signedIn",
      profile: accountProfile({
        id: "principal",
        displayName: "Example User",
        email: "user@example.com",
      }),
    }),
  );
  fixture.handle("account/logout", async () => {
    fixture.publish(accountSnapshot());
    return { account: fixture.snapshot, remoteRevocationConfirmed: false };
  });
  const onOpen = vi.fn<(section: "general" | "account" | "security") => void>();
  const onNotice = vi.fn<(message: string) => void>();
  renderAccount(fixture, <AccountMenu onOpen={onOpen} onNotice={onNotice} />);
  const user = userEvent.setup();
  await user.click(await screen.findByRole("button", { name: "Account: Example User" }));
  expect(screen.getByText("user@example.com")).toBeDefined();
  await user.click(screen.getByRole("menuitem", { name: "Account security" }));
  expect(onOpen).toHaveBeenCalledWith("security");
  await user.click(screen.getByRole("button", { name: "Account: Example User" }));
  await user.click(screen.getByRole("menuitem", { name: "Sign out" }));
  await waitFor(() => {
    expect(onNotice).toHaveBeenCalledWith(
      "Signed out locally. Remote revocation could not be confirmed.",
    );
  });
  expect(fixture.calls("account/logout")).toEqual([{ expectedAccountId: "principal" }]);
});
it("discards a previous account's delayed logout feedback after a principal change", async () => {
  const fixture = new AccountFixture(
    accountSnapshot({ status: "signedIn", profile: accountProfile({ displayName: "Account A" }) }),
  );
  const receipt = deferred<AccountMethods["account/logout"]["result"]>();
  fixture.handle("account/logout", async () => await receipt.promise);
  const onNotice = vi.fn<(message: string) => void>();
  renderAccount(
    fixture,
    <AccountMenu
      onOpen={vi.fn<(section: "general" | "account" | "security") => void>()}
      onNotice={onNotice}
    />,
  );
  await userEvent.click(await screen.findByRole("button", { name: "Account: Account A" }));
  await userEvent.click(screen.getByRole("menuitem", { name: "Sign out" }));
  act(() => {
    fixture.publish(
      accountSnapshot({
        status: "signedIn",
        profile: accountProfile({ id: "account-b", displayName: "Account B" }),
      }),
    );
  });
  await screen.findByRole("button", { name: "Account: Account B" });
  await act(async () => {
    receipt.resolve({ account: accountSnapshot(), remoteRevocationConfirmed: false });
    await receipt.promise;
  });
  await waitFor(() => {
    expect(
      screen.getByRole("button", { name: "Account: Account B" }).getAttribute("aria-busy"),
    ).toBe("false");
  });
  expect(onNotice).not.toHaveBeenCalled();
  expect(fixture.calls("account/logout")).toEqual([{ expectedAccountId: "account-a" }]);
});
