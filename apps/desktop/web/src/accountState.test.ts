// Copyright 2026 Syntropika
// SPDX-License-Identifier: Apache-2.0
import { expect, it, vi, afterEach } from "vitest";
import { AccountStore } from "./accountState";
const transport = vi.hoisted(() => ({ request: vi.fn(), native: vi.fn(() => true) }));
vi.mock("./account", () => ({ accountRequest: transport.request }));
vi.mock("./clientTransport", () => ({ nativeClient: transport.native }));
afterEach(() => { vi.clearAllMocks(); vi.useRealTimers(); });
const account = { status: "signedOut", generation: 0, profile: null, persistence: "secureVault", loginId: null };
const observed = (ownerId: string, revision: number, status = "signedOut") => ({ cursor: { ownerId, revision }, account: { ...account, status } });
it("shares one held observation and drops results after the last view closes", async () => {
  let complete!: (value: unknown) => void;
  transport.request.mockImplementation((method) => method === "account/read" ? Promise.resolve(account) : new Promise((resolve) => { complete = resolve; }));
  const store = new AccountStore();
  const a = store.subscribe(vi.fn()), b = store.subscribe(vi.fn());
  await vi.waitFor(() => expect(transport.request).toHaveBeenCalledWith("account/observe", { cursor: undefined }));
  expect(transport.request.mock.calls.filter(([method]) => method === "account/observe")).toHaveLength(1);
  a(); b(); complete(observed("first", 0)); await Promise.resolve();
  expect(store.getSnapshot().account).toBeUndefined();
});
it("rejects late revisions and observations from an owner replaced after restart", async () => {
  const store = new AccountStore();
  for (const value of [observed("first", 4, "signedIn"), observed("first", 2), observed("second", 0), observed("first", 5, "signedIn")]) {
    transport.request.mockResolvedValueOnce(value);
    await store.reconcile();
  }
  expect(store.getSnapshot().account?.status).toBe("signedOut");
});
it("reattaches after observation fails without replaying a mutation", async () => {
  vi.useFakeTimers();
  transport.request.mockImplementation((method) => method === "account/read" ? Promise.resolve(account) : Promise.reject(Error("owner stopped")));
  const store = new AccountStore(), close = store.subscribe(vi.fn());
  await vi.advanceTimersByTimeAsync(1);
  expect(store.getSnapshot().error).toContain("owner stopped");
  await vi.advanceTimersByTimeAsync(2000);
  expect(transport.request.mock.calls.filter(([method]) => method === "account/read")).toHaveLength(2);
  expect(transport.request.mock.calls.every(([method]) => method === "account/read" || method === "account/observe")).toBe(true);
  close();
});
it("keeps a revalidation failure visible while receiving retained account state", async () => {
  let observes = 0;
  transport.request.mockImplementation((method) => {
    if (method === "account/read") return Promise.reject(Error("Account service is unavailable"));
    if (observes++ === 0) return Promise.resolve(observed("owner", 0, "signedIn"));
    return new Promise(() => {});
  });
  const store = new AccountStore(), close = store.subscribe(vi.fn());
  await vi.waitFor(() => expect(store.getSnapshot().account?.status).toBe("signedIn"));
  expect(store.getSnapshot().error).toBe("Account service is unavailable");
  close();
});
