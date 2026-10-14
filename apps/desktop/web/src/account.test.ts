// Copyright 2026 Syntropika
// SPDX-License-Identifier: Apache-2.0
import { beforeEach, expect, it, vi } from "vitest";
import { accountRequest } from "./account";
const transport = vi.hoisted(() => ({ clientRequest: vi.fn(), nativeClient: vi.fn(() => true) }));
vi.mock("./clientTransport", () => transport);
beforeEach(() => { vi.clearAllMocks(); transport.nativeClient.mockReturnValue(true); });
it("sends account operations directly without consulting any execution node", async () => {
  transport.clientRequest.mockResolvedValue(new Response(JSON.stringify({ result: { status: "signedOut" } })));
  expect(await accountRequest("account/read", {})).toEqual({ status: "signedOut" });
  expect(transport.clientRequest).toHaveBeenCalledTimes(1);
  expect(transport.clientRequest.mock.calls[0][0]).toBe("/client/rpc");
  expect(transport.clientRequest.mock.calls[0][1].method).toBe("account/read");
  expect(transport.clientRequest.mock.calls[0][1].expectedRuntime).toBeUndefined();
});
it("propagates errors without retrying a mutation", async () => {
  transport.clientRequest.mockResolvedValue(new Response(JSON.stringify({ error: { message: "Profile changed" } })));
  await expect(accountRequest("account/logout", {})).rejects.toThrow("Profile changed");
  expect(transport.clientRequest).toHaveBeenCalledTimes(1);
});
it("does not request credentials from a browser presentation", async () => {
  transport.nativeClient.mockReturnValue(false);
  await expect(accountRequest("account/read", {})).rejects.toThrow("desktop app");
  expect(transport.clientRequest).not.toHaveBeenCalled();
});
