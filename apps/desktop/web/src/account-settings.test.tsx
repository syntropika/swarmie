// @vitest-environment happy-dom
// Copyright 2026 Syntropika
// SPDX-License-Identifier: Apache-2.0
import { render, screen, waitFor, cleanup } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { AccountSettings } from "./AccountSettings";
const request = vi.hoisted(() => vi.fn());
const presentation = vi.hoisted(() => ({ value: { account: undefined as any, error: "" }, listeners: new Set<() => void>() }));
function publish(account: any) { presentation.value = { account, error: "" }; for (const listener of presentation.listeners) listener(); }
vi.mock("./accountState", async () => {
  const { useSyncExternalStore } = await import("react");
  const reconcile = async () => { const account = await request("account/read", {}); publish(account); return account; };
  return { useAccount: () => useSyncExternalStore((listener) => { presentation.listeners.add(listener); return () => { presentation.listeners.delete(listener); }; }, () => presentation.value), accountStore: { reconcile, refresh: reconcile } };
});
vi.mock("./account", () => ({ accountRequest: request }));
vi.mock("./clientTransport", () => ({ nativeClient: () => true }));
const signedOut = { status: "signedOut", profile: null, loginId: null, persistence: "secureVault", generation: 0 };
beforeEach(() => { request.mockReset(); request.mockResolvedValue(signedOut); publish(signedOut); });
afterEach(cleanup);
it("requires an explicit choice for session-only sign-in and does not cancel on unmount", async () => {
  const user = userEvent.setup(); const view = render(<AccountSettings />);
  await screen.findByRole("button", { name: "Sign in" });
  await user.click(screen.getByRole("switch"));
  request.mockImplementation(async (method) => method === "account/login/start" ? { loginId: "login", authorizationUrl: "https://accounts.example.com/auth/native/authorize" } : { ...signedOut, status: "signingIn", loginId: "login" });
  await user.click(screen.getByRole("button", { name: "Sign in" }));
  await screen.findByRole("button", { name: "Cancel sign-in" });
  expect(request).toHaveBeenCalledWith("account/login/start", { sessionOnly: true });
  view.unmount(); expect(request.mock.calls.some(([method]) => method === "account/login/cancel")).toBe(false);
});
it("retains a mutation receipt for an explicit retry after an uncertain save", async () => {
  const profile = { id: "user", displayName: "Original", email: "test@example.com", emailVerified: true, revision: 4 };
  request.mockImplementation(async (method) => { if (method === "account/profile/update") throw Error("Request outcome is unknown"); return { ...signedOut, status: "signedIn", profile }; });
  publish({ ...signedOut, status: "signedIn", profile });
  const user = userEvent.setup(); render(<AccountSettings />);
  const input = await screen.findByRole("textbox", { name: "Display name" }); await waitFor(() => expect((input as HTMLInputElement).value).toBe("Original"));
  await user.clear(input); await user.type(input, "Updated"); await user.click(screen.getByRole("button", { name: "Save profile" }));
  await screen.findByRole("alert"); await user.click(screen.getByRole("button", { name: "Save profile" }));
  await waitFor(() => expect(request.mock.calls.filter(([method]) => method === "account/profile/update")).toHaveLength(2));
  const calls = request.mock.calls.filter(([method]) => method === "account/profile/update"); expect(calls[0][1]).toEqual(calls[1][1]);
});

it("preserves an unsaved name when another window updates the profile", async () => {
  const profile = { id: "user", displayName: "Original", email: "test@example.com", emailVerified: true, avatarUrl: null, revision: 4 };
  publish({ ...signedOut, status: "signedIn", profile });
  const user = userEvent.setup(); render(<AccountSettings />);
  const input = await screen.findByRole("textbox", { name: "Display name" });
  await user.clear(input); await user.type(input, "My unsaved draft");
  const { act } = await import("@testing-library/react");
  act(() => publish({ ...signedOut, status: "signedIn", profile: { ...profile, displayName: "Saved elsewhere", revision: 5 } }));
  expect((input as HTMLInputElement).value).toBe("My unsaved draft");
  request.mockImplementation(async (method) => { if (method === "account/profile/update") throw Error("Profile changed; read it again before saving"); return { ...signedOut, status: "signedIn", profile }; });
  await user.click(screen.getByRole("button", { name: "Save profile" }));
  expect(request.mock.calls.find(([method]) => method === "account/profile/update")?.[1].expectedRevision).toBe(4);
  expect((input as HTMLInputElement).value).toBe("My unsaved draft");
  act(() => publish({ ...signedOut, status: "signedIn", profile: { ...profile, id: "another-user", displayName: "Another account", revision: 0 } }));
  await waitFor(() => expect((input as HTMLInputElement).value).toBe("Another account"));
});
it("can reopen a pending browser sign-in after the settings view was closed", async () => {
  publish({ ...signedOut, status: "signingIn", loginId: "login", authorizationUrl: "https://accounts.example.com/auth/native/authorize?request=pending" });
  const view = render(<AccountSettings />);
  expect(screen.getByRole("link", { name: "Open sign-in in browser" }).getAttribute("href")).toContain("request=pending");
  view.unmount(); render(<AccountSettings />);
  expect(screen.getByRole("link", { name: "Open sign-in in browser" }).getAttribute("href")).toContain("request=pending");
  expect(request).not.toHaveBeenCalled();
});
