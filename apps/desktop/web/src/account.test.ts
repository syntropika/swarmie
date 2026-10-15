// @vitest-environment happy-dom
// Copyright 2026 Syntropika
// SPDX-License-Identifier: Apache-2.0
import { beforeEach, afterEach, expect, it, vi } from "vitest";

import { accountRequest, accountReceiptId } from "~/web/src/account";
import { accountSnapshot } from "~/web/src/account-test-fixture";
import { record } from "~/web/src/checked-json";
import { nativeRequestFixture } from "~/web/src/test-fixtures";

let native: ReturnType<typeof nativeRequestFixture> | undefined;
const request = vi.fn<(method: string, params: unknown) => Promise<unknown>>();
beforeEach(() => {
  request.mockReset();
});
afterEach(() => {
  native?.restore();
  native = undefined;
  vi.restoreAllMocks();
});
it("sends account operations directly without consulting any execution node", async () => {
  const account = accountSnapshot();
  request.mockResolvedValue(account);
  native = nativeRequestFixture(request);
  expect(await accountRequest("account/read", {})).toEqual(account);
  expect(request).toHaveBeenCalledExactlyOnceWith("account/read", {});
  const payload = record(native.invoke.mock.calls.at(0)?.at(1));
  expect(payload.path).toBe("/client/rpc");
  expect(record(payload.request)).not.toHaveProperty("expectedRuntime");
});
it("propagates native errors without retrying a mutation", async () => {
  request.mockRejectedValue(new Error("Profile changed"));
  native = nativeRequestFixture(request);
  await expect(accountRequest("account/logout", {})).rejects.toThrow("Profile changed");
  expect(request).toHaveBeenCalledTimes(1);
});
it("does not request credentials from a browser presentation", async () => {
  await expect(accountRequest("account/read", {})).rejects.toThrow("desktop app");
  expect(request).not.toHaveBeenCalled();
});
it("creates time-bound receipt identifiers accepted by the server contract", () => {
  const now = 1_780_000_000_123;
  const clock = vi.spyOn(Date, "now").mockReturnValue(now);
  const ids = new Set(Array.from({ length: 100 }, accountReceiptId));
  expect(ids.size).toBe(100);
  for (const id of ids) {
    expect(id).toMatch(/^[\da-f]{8}-[\da-f]{4}-7[\da-f]{3}-[89ab][\da-f]{3}-[\da-f]{12}$/u);
    expect(Number.parseInt(id.replaceAll("-", "").slice(0, 12), 16)).toBe(now);
  }
  clock.mockRestore();
});
