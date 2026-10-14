// @vitest-environment happy-dom
// Copyright 2026 Syntropika
// SPDX-License-Identifier: Apache-2.0
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { AccountSecurity } from "./AccountSecurity";
const fixture = vi.hoisted(() => ({ state: { account: undefined as any, error: "" }, request: vi.fn(), reconcile: vi.fn() }));
vi.mock("./accountState", () => ({ useAccount: () => fixture.state, accountStore: { reconcile: fixture.reconcile } }));
vi.mock("./account", () => ({ accountRequest: fixture.request }));
const session = (id: string) => ({ id, installationId: "installation-id", authenticatedAt: "2026-10-14T10:00:00Z", createdAt: "2026-10-14T10:00:00Z", lastUsedAt: "2026-10-14T11:00:00Z", expiresAt: "2026-11-13T10:00:00Z" });
const page = { current: session("current-session"), others: [session("other-session")], nextCursor: null };
beforeEach(() => {
  vi.clearAllMocks();
  fixture.state = { account: { status: "signedIn", profile: { id: "account-a", email: "user@example.test" }, persistence: "secureVault" }, error: "" };
  fixture.reconcile.mockResolvedValue({});
});
afterEach(cleanup);
it("revokes only the selected session with the displayed account intent and refreshes metadata", async () => {
  fixture.request.mockResolvedValueOnce(page).mockResolvedValueOnce({ status: "signedIn" }).mockResolvedValueOnce({ ...page, others: [] });
  render(<AccountSecurity onProfile={vi.fn()} />);
  const user = userEvent.setup();
  await user.click(await screen.findByRole("button", { name: "Sign out session other-se" }));
  await screen.findByText("No other active native sessions.");
  expect(fixture.request).toHaveBeenNthCalledWith(2, "account/sessions/revoke", { expectedAccountId: "account-a", sessionId: "other-session" });
  expect(screen.getByRole("status").textContent).toContain("selected native session");
});
it("requires an explicit confirmation for global sign-out and does not claim success on failure", async () => {
  fixture.request.mockResolvedValueOnce(page).mockRejectedValueOnce(new Error("Account service is unavailable"));
  render(<AccountSecurity onProfile={vi.fn()} />);
  const user = userEvent.setup();
  await screen.findByRole("button", { name: "Sign out session other-se" });
  await user.click(screen.getByRole("button", { name: "Sign out all native sessions" }));
  expect(fixture.request).toHaveBeenCalledTimes(1);
  expect(document.activeElement).toBe(screen.getByRole("button", { name: "Cancel" }));
  await user.click(screen.getByRole("button", { name: "Confirm sign out everywhere" }));
  expect((await screen.findByRole("alert")).textContent).toContain("unavailable");
  expect(fixture.request).toHaveBeenNthCalledWith(2, "account/sessions/revoke-all", { expectedAccountId: "account-a" });
  expect(screen.queryByRole("status")).toBeNull();
  expect(document.activeElement).toBe(screen.getByRole("button", { name: "Refresh session state" }));
  expect(fixture.reconcile).toHaveBeenCalledTimes(1);
});
it("offers a retry after a failed session read without hiding local sign-out", async () => {
  fixture.request.mockRejectedValueOnce(new Error("Service unavailable")).mockResolvedValueOnce(page);
  render(<AccountSecurity onProfile={vi.fn()} />);
  const user = userEvent.setup();
  await screen.findByRole("alert");
  expect(screen.getByRole("button", { name: "Sign out of this installation" })).toBeDefined();
  await user.click(screen.getByRole("button", { name: "Refresh sessions" }));
  await screen.findByRole("button", { name: "Sign out session other-se" });
  expect(screen.queryByRole("alert")).toBeNull();
});
it("discards a former account's delayed session response after the observed identity changes", async () => {
  let finish!: (value: unknown) => void;
  fixture.request.mockReturnValueOnce(new Promise((resolve) => { finish = resolve; })).mockResolvedValueOnce({ ...page, others: [] });
  const view = render(<AccountSecurity onProfile={vi.fn()} />);
  fixture.state.account = { ...fixture.state.account, profile: { id: "account-b", email: "other@example.test" } };
  view.rerender(<AccountSecurity onProfile={vi.fn()} />);
  await screen.findByText("No other active native sessions.");
  finish(page);
  await waitFor(() => expect(screen.queryByRole("button", { name: "Sign out session other-se" })).toBeNull());
  expect(fixture.request).toHaveBeenNthCalledWith(2, "account/sessions/read", { expectedAccountId: "account-b" });
});
