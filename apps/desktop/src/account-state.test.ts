// Copyright 2026 Syntropika
// SPDX-License-Identifier: Apache-2.0
import type { AccountObservation, AccountSnapshot } from "@swarmie/contracts";
import { expect, it, vi, afterEach } from "vitest";

import { AccountStore } from "~/src/account-state";
import type { AccountIO } from "~/src/account-state";
import { accountSnapshot, accountProfile } from "~/src/account-test-fixture";
import { deferred } from "~/src/test-fixtures";

afterEach(() => {
  vi.useRealTimers();
});
function observed(
  ownerId: string,
  revision: number,
  status: AccountSnapshot["status"] = "signedOut",
): AccountObservation {
  return {
    cursor: { ownerId, revision },
    account: accountSnapshot({ status, profile: status === "signedIn" ? accountProfile() : null }),
  };
}
function accountOwner(request: AccountIO["request"]) {
  return new AccountStore({ request, native: () => true });
}
it("shares one held observation and drops results after the last view closes", async () => {
  const receipt = deferred<AccountObservation>();
  const request = vi.fn<AccountIO["request"]>(async (method) =>
    method === "account/read" ? accountSnapshot() : await receipt.promise,
  );
  const store = accountOwner(request);
  const a = store.subscribe(vi.fn<() => void>());
  const b = store.subscribe(vi.fn<() => void>());
  await vi.waitFor(() => {
    expect(request).toHaveBeenCalledWith("account/observe", { cursor: undefined });
  });
  expect(request.mock.calls.filter(([method]) => method === "account/observe")).toHaveLength(1);
  a();
  b();
  receipt.resolve(observed("first", 0));
  await receipt.promise;
  expect(store.getSnapshot().account).toBeUndefined();
});
it("rejects late revisions and observations from an owner replaced after restart", async () => {
  const request = vi.fn<AccountIO["request"]>();
  const store = accountOwner(request);
  for (const value of [
    observed("first", 4, "signedIn"),
    observed("first", 2),
    observed("second", 0),
    observed("first", 5, "signedIn"),
  ]) {
    request.mockResolvedValueOnce(value);
    // Each native owner observation follows the previous retained cursor and must remain ordered.
    // oxlint-disable-next-line no-await-in-loop
    await store.reconcile();
  }
  expect(store.getSnapshot().account?.status).toBe("signedOut");
});
it("reattaches after observation fails without replaying a mutation", async () => {
  vi.useFakeTimers();
  const request = vi.fn<AccountIO["request"]>(async (method) => {
    if (method === "account/read") {
      return accountSnapshot();
    }
    throw new Error("owner stopped");
  });
  const store = accountOwner(request);
  const close = store.subscribe(vi.fn<() => void>());
  await vi.advanceTimersByTimeAsync(1);
  expect(store.getSnapshot().error).toContain("owner stopped");
  await vi.advanceTimersByTimeAsync(2000);
  expect(request.mock.calls.filter(([method]) => method === "account/read")).toHaveLength(2);
  expect(
    request.mock.calls.every(
      ([method]) => method === "account/read" || method === "account/observe",
    ),
  ).toBe(true);
  close();
});
it("keeps a revalidation failure visible while receiving retained account state", async () => {
  const pending = deferred<AccountObservation>();
  let observes = 0;
  const request = vi.fn<AccountIO["request"]>(async (method) => {
    if (method === "account/read") {
      throw new Error("Account service is unavailable");
    }
    observes += 1;
    return observes === 1 ? observed("owner", 0, "signedIn") : await pending.promise;
  });
  const store = accountOwner(request);
  const close = store.subscribe(vi.fn<() => void>());
  await vi.waitFor(() => {
    expect(store.getSnapshot().account?.status).toBe("signedIn");
  });
  expect(store.getSnapshot().error).toBe("Account service is unavailable");
  close();
  pending.resolve(observed("owner", 1));
  await pending.promise;
});
