// @vitest-environment happy-dom
// Copyright 2026 Syntropika
// SPDX-License-Identifier: Apache-2.0
import { render, screen, waitFor, cleanup } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { AccountSettings } from "./AccountSettings";
const request = vi.hoisted(() => vi.fn());
vi.mock("./account", () => ({ accountRequest: request }));
vi.mock("./clientTransport", () => ({ nativeClient: () => true }));
const signedOut = { status: "signedOut", profile: null, loginId: null, persistence: "secureVault", generation: 0 };
beforeEach(() => { request.mockReset(); request.mockResolvedValue(signedOut); });
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
  const user = userEvent.setup(); render(<AccountSettings />);
  const input = await screen.findByRole("textbox", { name: "Display name" }); await waitFor(() => expect((input as HTMLInputElement).value).toBe("Original"));
  await user.clear(input); await user.type(input, "Updated"); await user.click(screen.getByRole("button", { name: "Save profile" }));
  await screen.findByRole("alert"); await user.click(screen.getByRole("button", { name: "Save profile" }));
  await waitFor(() => expect(request.mock.calls.filter(([method]) => method === "account/profile/update")).toHaveLength(2));
  const calls = request.mock.calls.filter(([method]) => method === "account/profile/update"); expect(calls[0][1]).toEqual(calls[1][1]);
});
