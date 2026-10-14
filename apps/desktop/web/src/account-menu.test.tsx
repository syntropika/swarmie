// @vitest-environment happy-dom
// Copyright 2026 Syntropika
// SPDX-License-Identifier: Apache-2.0
import { render, screen, cleanup, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { AccountMenu } from "./AccountMenu";
const fixture = vi.hoisted(() => ({ state: { account: undefined as any, error: "" }, request: vi.fn(), reconcile: vi.fn() }));
vi.mock("./accountState", () => ({ useAccount: () => fixture.state, accountStore: { reconcile: fixture.reconcile } }));
vi.mock("./account", () => ({ accountRequest: fixture.request }));
vi.mock("./clientTransport", () => ({ nativeClient: () => true }));
beforeEach(() => { vi.clearAllMocks(); fixture.state = { account: { status: "signedOut", profile: null }, error: "" }; });
afterEach(cleanup);
it("keeps settings available without login and directs sign-in to the account page", async () => {
  const onOpen = vi.fn(); render(<AccountMenu onOpen={onOpen} onNotice={vi.fn()} />);
  const user = userEvent.setup(); await user.click(screen.getByRole("button", { name: "Account: Sign in" }));
  await user.click(screen.getByRole("menuitem", { name: "Settings" })); expect(onOpen).toHaveBeenCalledWith("general");
  await user.click(screen.getByRole("button", { name: "Account: Sign in" }));
  await user.click(screen.getByRole("menuitem", { name: "Sign in" })); expect(onOpen).toHaveBeenCalledWith("account");
});
it("shows identity, opens account security and reports uncertain remote revocation", async () => {
  fixture.state.account = { status: "signedIn", profile: { id: "principal", displayName: "Example User", email: "user@example.com", avatarUrl: null } };
  fixture.request.mockResolvedValue({ remoteRevocationConfirmed: false }); fixture.reconcile.mockResolvedValue({});
  const onOpen = vi.fn(), onNotice = vi.fn(); render(<AccountMenu onOpen={onOpen} onNotice={onNotice} />);
  const user = userEvent.setup(); await user.click(screen.getByRole("button", { name: "Account: Example User" }));
  expect(screen.getByText("user@example.com")).toBeDefined();
  await user.click(screen.getByRole("menuitem", { name: "Account security" })); expect(onOpen).toHaveBeenCalledWith("security");
  await user.click(screen.getByRole("button", { name: "Account: Example User" }));
  await user.click(screen.getByRole("menuitem", { name: "Sign out" }));
  await waitFor(() => expect(onNotice).toHaveBeenCalledWith("Signed out locally. Remote revocation could not be confirmed."));
  expect(fixture.request).toHaveBeenCalledWith("account/logout", { expectedAccountId: "principal" });
  expect(fixture.request).toHaveBeenCalledTimes(1);
});
