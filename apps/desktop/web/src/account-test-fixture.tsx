// Copyright 2026 Syntropika
// SPDX-License-Identifier: Apache-2.0
import type {
  AccountMethods,
  AccountObservation,
  AccountProfile,
  AccountSession,
  AccountSnapshot,
} from "@swarmie/contracts";
import { render } from "@testing-library/react";
import type { ReactNode } from "react";
import { vi } from "vitest";

import { AccountProvider } from "~/web/src/account-provider";
import { AccountStore } from "~/web/src/account-state";
import type { AccountIO } from "~/web/src/account-state";
import { isRecord } from "~/web/src/checked-json";
import { NodeQueryBoundary } from "~/web/src/query-boundary";
import { deferred } from "~/web/src/test-fixtures";

type Method = keyof AccountMethods;
export function accountSnapshot(patch: Partial<AccountSnapshot> = {}): AccountSnapshot {
  return {
    status: "signedOut",
    generation: 0,
    profile: null,
    persistence: "secureVault",
    loginId: null,
    configured: true,
    ...patch,
  };
}
export function accountProfile(patch: Partial<AccountProfile> = {}): AccountProfile {
  return {
    id: "account-a",
    email: "user@example.test",
    emailVerified: true,
    displayName: "Original",
    avatarUrl: null,
    revision: 4,
    ...patch,
  };
}
export function accountSession(id: string): AccountSession {
  return {
    id,
    installationId: "installation-id",
    authenticatedAt: "2026-10-10T10:00:00Z",
    createdAt: "2026-10-14T10:00:00Z",
    lastUsedAt: "2026-10-14T11:00:00Z",
    expiresAt: "2026-11-13T10:00:00Z",
  };
}
/** The actual account observer runs against a held native I/O receipt, with no module mocks. */
export class AccountFixture {
  private revision = 0;
  private readonly pending: ReturnType<typeof deferred<AccountObservation>>[] = [];
  private readonly handlers = new Map<Method, (params: unknown) => Promise<unknown>>();
  snapshot: AccountSnapshot;
  constructor(snapshot: AccountSnapshot = accountSnapshot()) {
    this.snapshot = snapshot;
  }
  request = vi.fn<AccountIO["request"]>(async (method, params) => {
    const handler = this.handlers.get(method);
    if (handler !== undefined) {
      return await handler(params);
    }
    if (method === "account/read") {
      return this.snapshot;
    }
    if (method !== "account/observe") {
      throw new Error(`Unexpected fixture request: ${method}`);
    }
    const cursor = isRecord(params) ? params.cursor : undefined;
    if (
      isRecord(cursor) &&
      cursor.ownerId === "fixture-owner" &&
      typeof cursor.revision === "number" &&
      cursor.revision >= this.revision
    ) {
      const receipt = deferred<AccountObservation>();
      this.pending.push(receipt);
      return await receipt.promise;
    }
    return this.observation();
  });
  readonly store = new AccountStore({ request: this.request, native: () => true });
  handle(method: Method, handler: (params: unknown) => Promise<unknown>) {
    this.handlers.set(method, handler);
  }
  calls(method: Method) {
    return this.request.mock.calls.filter(([name]) => name === method).map(([, params]) => params);
  }
  publish(snapshot: AccountSnapshot) {
    this.snapshot = snapshot;
    this.revision += 1;
    for (const receipt of this.pending.splice(0)) {
      receipt.resolve(this.observation());
    }
  }
  private observation(): AccountObservation {
    return {
      cursor: { ownerId: "fixture-owner", revision: this.revision },
      account: this.snapshot,
    };
  }
}
export function renderAccount(fixture: AccountFixture, view: ReactNode) {
  function AccountTestBoundary({ children }: { children: ReactNode }) {
    return (
      <NodeQueryBoundary>
        <AccountProvider store={fixture.store}>{children}</AccountProvider>
      </NodeQueryBoundary>
    );
  }
  return render(view, { wrapper: AccountTestBoundary });
}

export function accountFixtureValue<T>(value: T | null | undefined): T {
  if (value === null || value === undefined) {
    throw new Error("Expected an account fixture value.");
  }
  return value;
}
