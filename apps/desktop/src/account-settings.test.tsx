// @vitest-environment happy-dom
// Copyright 2026 Syntropika
// SPDX-License-Identifier: Apache-2.0
import { act, screen, waitFor, cleanup } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, it } from "vitest";

import { AccountSettings } from "~/src/account-settings";
import {
  AccountFixture,
  accountSnapshot,
  accountProfile,
  renderAccount,
  accountFixtureValue,
} from "~/src/account-test-fixture";
import { deferred } from "~/src/test-fixtures";

afterEach(cleanup);
it("requires an explicit choice for session-only sign-in and does not cancel on unmount", async () => {
  const fixture = new AccountFixture();
  fixture.handle("account/login/start", async () => {
    fixture.publish(
      accountSnapshot({
        status: "signingIn",
        loginId: "login",
        persistence: "session",
        authorizationUrl: "https://accounts.example.com/auth/native/authorize",
      }),
    );
    return {
      loginId: "login",
      authorizationUrl: "https://accounts.example.com/auth/native/authorize",
      expiresIn: 300,
      persistence: "session",
    };
  });
  const view = renderAccount(fixture, <AccountSettings />);
  const user = userEvent.setup();
  await screen.findByRole("button", { name: "Sign in" });
  await user.click(screen.getByRole("switch"));
  await user.click(screen.getByRole("button", { name: "Sign in" }));
  await screen.findByRole("button", { name: "Cancel sign-in" });
  expect(fixture.calls("account/login/start")).toEqual([{ sessionOnly: true }]);
  view.unmount();
  expect(fixture.calls("account/login/cancel")).toEqual([]);
});
it("retains a mutation receipt for an explicit retry after an uncertain save", async () => {
  const fixture = new AccountFixture(
    accountSnapshot({ status: "signedIn", profile: accountProfile() }),
  );
  fixture.handle("account/profile/update", async () => {
    throw new Error("Request outcome is unknown");
  });
  const user = userEvent.setup();
  renderAccount(fixture, <AccountSettings />);
  const input = await screen.findByRole<HTMLInputElement>("textbox", { name: "Display name" });
  await user.clear(input);
  await user.type(input, "Updated");
  await user.click(screen.getByRole("button", { name: "Save profile" }));
  await screen.findByText("Request outcome is unknown");
  expect(fixture.calls("account/profile/update")).toHaveLength(1);
  await user.click(screen.getByRole("button", { name: "Save profile" }));
  await waitFor(() => {
    expect(fixture.calls("account/profile/update")).toHaveLength(2);
  });
  const calls = fixture.calls("account/profile/update");
  expect(calls.at(0)).toEqual(calls.at(1));
});
it("preserves a dirty CAS revision across observations and replaces the editor on a principal change", async () => {
  const profile = accountProfile();
  const fixture = new AccountFixture(accountSnapshot({ status: "signedIn", profile }));
  fixture.handle("account/profile/update", async () => {
    throw new Error("Profile changed; read it again before saving");
  });
  const user = userEvent.setup();
  renderAccount(fixture, <AccountSettings />);
  const input = await screen.findByRole<HTMLInputElement>("textbox", { name: "Display name" });
  await user.clear(input);
  await user.type(input, "My unsaved draft");
  act(() => {
    fixture.publish(
      accountSnapshot({
        status: "signedIn",
        profile: { ...profile, displayName: "Saved elsewhere", revision: 5 },
      }),
    );
  });
  await waitFor(() => {
    expect(screen.getByRole<HTMLInputElement>("textbox", { name: "Display name" }).value).toBe(
      "My unsaved draft",
    );
  });
  await user.click(screen.getByRole("button", { name: "Save profile" }));
  await screen.findByText("Profile changed; read it again before saving");
  expect(fixture.calls("account/profile/update").at(0)).toHaveProperty("expectedRevision", 4);
  act(() => {
    fixture.publish(
      accountSnapshot({
        status: "signedIn",
        profile: accountProfile({ id: "account-b", displayName: "Another account", revision: 0 }),
      }),
    );
  });
  await waitFor(() => {
    expect(screen.getByRole<HTMLInputElement>("textbox", { name: "Display name" }).value).toBe(
      "Another account",
    );
  });
});
it("refreshes an untouched name without manufacturing a dirty revision", async () => {
  const profile = accountProfile();
  const fixture = new AccountFixture(accountSnapshot({ status: "signedIn", profile }));
  renderAccount(fixture, <AccountSettings />);
  await screen.findByRole<HTMLInputElement>("textbox", { name: "Display name" });
  act(() => {
    fixture.publish(
      accountSnapshot({
        status: "signedIn",
        profile: { ...profile, displayName: "Saved elsewhere", revision: 5 },
      }),
    );
  });
  await waitFor(() => {
    expect(screen.getByRole<HTMLInputElement>("textbox", { name: "Display name" }).value).toBe(
      "Saved elsewhere",
    );
  });
  expect(screen.getByRole<HTMLButtonElement>("button", { name: "Save profile" }).disabled).toBe(
    true,
  );
});
it("captures the initiating account and revision before reading an image", async () => {
  const fixture = new AccountFixture(
    accountSnapshot({ status: "signedIn", profile: accountProfile() }),
  );
  const bytes = deferred<ArrayBuffer>();
  const file = new File(["image"], "profile.png", { type: "image/png" });
  Object.defineProperty(file, "arrayBuffer", { value: async () => await bytes.promise });
  fixture.handle("account/profile/avatar/update", async () => fixture.snapshot);
  renderAccount(fixture, <AccountSettings />);
  const input = await screen.findByLabelText<HTMLInputElement>("Profile image");
  await userEvent.upload(input, file);
  act(() => {
    fixture.publish(
      accountSnapshot({
        status: "signedIn",
        profile: accountProfile({ id: "account-b", revision: 0, email: "b@example.test" }),
      }),
    );
  });
  await waitFor(() => {
    expect(screen.getByText(/b@example\.test/u)).toBeDefined();
  });
  await act(async () => {
    bytes.resolve(new Uint8Array([1, 2, 3]).buffer);
    await bytes.promise;
  });
  await waitFor(() => {
    expect(fixture.calls("account/profile/avatar/update")).toHaveLength(1);
  });
  const receipt = accountFixtureValue(fixture.calls("account/profile/avatar/update").at(0));
  expect(receipt).toHaveProperty("accountId", "account-a");
  expect(receipt).toHaveProperty("expectedRevision", 4);
});
it("can reopen a pending browser sign-in after the settings view was closed", async () => {
  const fixture = new AccountFixture(
    accountSnapshot({
      status: "signingIn",
      loginId: "login",
      authorizationUrl: "https://accounts.example.com/auth/native/authorize?request=pending",
    }),
  );
  const view = renderAccount(fixture, <AccountSettings />);
  const link = await screen.findByRole("link", { name: "Open sign-in in browser" });
  expect(link.getAttribute("href")).toContain("request=pending");
  view.unmount();
  renderAccount(fixture, <AccountSettings />);
  const reopenedLink = await screen.findByRole("link", { name: "Open sign-in in browser" });
  expect(reopenedLink.getAttribute("href")).toContain("request=pending");
  expect(fixture.calls("account/login/start")).toEqual([]);
  expect(fixture.calls("account/login/cancel")).toEqual([]);
});
