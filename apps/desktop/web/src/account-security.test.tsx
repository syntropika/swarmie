// @vitest-environment happy-dom
// Copyright 2026 Syntropika
// SPDX-License-Identifier: Apache-2.0
import type { AccountSessionPage } from "@swarmie/contracts";
import { act, cleanup, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, it, vi } from "vitest";

import { AccountSecurity } from "./account-security";
import {
  AccountFixture,
  accountSnapshot,
  accountProfile,
  accountSession,
  renderAccount,
  accountFixtureValue,
} from "./account-test-fixture";
import { record } from "./checked-json";
import { deferred } from "./test-fixtures";

const page: AccountSessionPage = {
  current: accountSession("current-session"),
  others: [accountSession("other-session")],
  nextCursor: null,
};
function signedIn() {
  return new AccountFixture(accountSnapshot({ status: "signedIn", profile: accountProfile() }));
}
function security(fixture: AccountFixture) {
  return renderAccount(fixture, <AccountSecurity onProfile={vi.fn<() => void>()} />);
}
afterEach(cleanup);
it("shows the authentication time when family creation happened later", async () => {
  const fixture = signedIn();
  fixture.handle("account/sessions/read", async () => page);
  security(fixture);
  await screen.findByRole("button", { name: "Sign out session other-se" });
  for (const label of screen.getAllByText("Signed in", { exact: true })) {
    const time = accountFixtureValue(
      accountFixtureValue(label.parentElement).querySelector("time"),
    );
    expect(time.getAttribute("datetime")).toBe(page.current.authenticatedAt);
    expect(time.textContent).toBe(
      new Date(page.current.authenticatedAt).toLocaleString(undefined, {
        dateStyle: "medium",
        timeStyle: "short",
      }),
    );
  }
});
it("revokes only the selected session with the displayed account intent and refreshes metadata", async () => {
  const fixture = signedIn();
  let currentPage = page;
  fixture.handle("account/sessions/read", async () => currentPage);
  fixture.handle("account/sessions/revoke", async () => {
    currentPage = { ...page, others: [] };
    return fixture.snapshot;
  });
  security(fixture);
  await userEvent.click(await screen.findByRole("button", { name: "Sign out session other-se" }));
  await screen.findByText("No other active native sessions.");
  expect(fixture.calls("account/sessions/revoke")).toEqual([
    { expectedAccountId: "account-a", sessionId: "other-session" },
  ]);
  expect(screen.getByRole("status").textContent).toContain("selected native session");
});
it("requires explicit confirmation for global sign-out and does not claim success on failure", async () => {
  const fixture = signedIn();
  fixture.handle("account/sessions/read", async () => page);
  fixture.handle("account/sessions/revoke-all", async () => {
    throw new Error("Account service is unavailable");
  });
  security(fixture);
  await screen.findByRole("button", { name: "Sign out session other-se" });
  await userEvent.click(screen.getByRole("button", { name: "Sign out all native sessions" }));
  expect(fixture.calls("account/sessions/revoke-all")).toEqual([]);
  expect(document.activeElement).toBe(screen.getByRole("button", { name: "Cancel" }));
  await userEvent.click(screen.getByRole("button", { name: "Confirm sign out everywhere" }));
  await screen.findAllByText(/Account service is unavailable/u);
  expect(fixture.calls("account/sessions/revoke-all")).toEqual([
    { expectedAccountId: "account-a" },
  ]);
  expect(
    screen.queryByText(
      "All native sessions are signed out. Your browser session remains available.",
    ),
  ).toBeNull();
  expect(document.activeElement).toBe(
    screen.getByRole("button", { name: "Refresh session state" }),
  );
});
it("offers a retry after a failed session read without hiding local sign-out", async () => {
  const fixture = signedIn();
  let reads = 0;
  fixture.handle("account/sessions/read", async () => {
    reads += 1;
    if (reads === 1) {
      throw new Error("Service unavailable");
    }
    return page;
  });
  security(fixture);
  await screen.findByRole("alert");
  expect(screen.getByRole("button", { name: "Sign out of this installation" })).toBeDefined();
  await userEvent.click(screen.getByRole("button", { name: "Refresh sessions" }));
  await screen.findByRole("button", { name: "Sign out session other-se" });
  expect(screen.queryByRole("alert")).toBeNull();
  expect(reads).toBe(2);
});
it("discards a former account's delayed session response after observed identity changes", async () => {
  const fixture = signedIn();
  const previous = deferred<AccountSessionPage>();
  fixture.handle("account/sessions/read", async (params) =>
    record(params).expectedAccountId === "account-a"
      ? await previous.promise
      : { ...page, others: [] },
  );
  security(fixture);
  await waitFor(() => {
    expect(fixture.calls("account/sessions/read")).toHaveLength(1);
  });
  act(() => {
    fixture.publish(
      accountSnapshot({
        status: "signedIn",
        profile: accountProfile({ id: "account-b", email: "other@example.test" }),
      }),
    );
  });
  await screen.findByText("No other active native sessions.");
  await act(async () => {
    previous.resolve(page);
    await previous.promise;
  });
  expect(screen.queryByRole("button", { name: "Sign out session other-se" })).toBeNull();
  expect(fixture.calls("account/sessions/read").at(1)).toEqual({ expectedAccountId: "account-b" });
});
it("does not display an earlier principal's delayed revoke failure on the replacement account", async () => {
  const fixture = signedIn();
  const revoke = deferred<unknown>();
  fixture.handle("account/sessions/read", async (params) =>
    record(params).expectedAccountId === "account-a" ? page : { ...page, others: [] },
  );
  fixture.handle("account/sessions/revoke", async () => await revoke.promise);
  security(fixture);
  await userEvent.click(await screen.findByRole("button", { name: "Sign out session other-se" }));
  act(() => {
    fixture.publish(
      accountSnapshot({
        status: "signedIn",
        profile: accountProfile({ id: "account-b", email: "other@example.test" }),
      }),
    );
  });
  await screen.findByText("other@example.test");
  await act(async () => {
    revoke.reject(new Error("Previous account revoke failed"));
    await expect(revoke.promise).rejects.toThrow("Previous account revoke failed");
  });
  expect(screen.queryByText(/Previous account revoke failed/u)).toBeNull();
  expect(fixture.calls("account/sessions/revoke")).toEqual([
    { expectedAccountId: "account-a", sessionId: "other-session" },
  ]);
});
