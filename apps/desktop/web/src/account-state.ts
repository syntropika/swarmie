import type {
  AccountMethods,
  AccountCursor,
  AccountObservation,
  AccountSnapshot,
} from "@swarmie/contracts";
// Copyright 2026 Syntropika
// SPDX-License-Identifier: Apache-2.0
import { createContext, useContext, useSyncExternalStore } from "react";

import { accountTransport } from "~/web/src/account";
import { decodeAccountResult } from "~/web/src/account-wire";
import { startAction, errorMessage } from "~/web/src/action-boundary";
import { nativeClient } from "~/web/src/client-transport";

export type AccountIO = {
  request: (method: keyof AccountMethods, params: unknown) => Promise<unknown>;
  native: () => boolean;
};
const nativeAccountIO: AccountIO = { request: accountTransport, native: nativeClient };
type AccountState = { account?: AccountSnapshot; error: string };
/** One observation per window, shared by navigation and settings and independent of node selection. */
export class AccountStore {
  constructor(private readonly io: AccountIO = nativeAccountIO) {}
  available = () => this.io.native();
  async request<M extends keyof AccountMethods>(
    method: M,
    params: AccountMethods[M]["params"],
  ): Promise<AccountMethods[M]["result"]> {
    return decodeAccountResult(method, await this.io.request(method, params));
  }
  private state: AccountState = { error: "" };
  private readonly listeners = new Set<() => void>();
  private cursor?: AccountCursor;
  private readonly retiredOwners = new Set<string>();
  private lifecycle = 0;
  private running = false;
  private validationError = "";
  private retry?: ReturnType<typeof setTimeout>;
  getSnapshot = () => this.state;
  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    if (!this.running && this.io.native()) {
      this.running = true;
      this.lifecycle += 1;
      const { lifecycle } = this;
      startAction(this.start(lifecycle), (actionError) => {
        this.publish({ ...this.state, error: errorMessage(actionError) });
      });
    }
    return () => {
      this.listeners.delete(listener);
      if (!this.listeners.size) {
        this.running = false;
        this.lifecycle += 1;
        clearTimeout(this.retry);
      }
    };
  };
  private publish(state: AccountState) {
    this.state = state;
    for (const listener of this.listeners) {
      listener();
    }
  }
  private apply(value: AccountObservation) {
    if (this.retiredOwners.has(value.cursor.ownerId)) {
      return;
    }
    if (
      this.cursor?.ownerId === value.cursor.ownerId &&
      this.cursor.revision > value.cursor.revision
    ) {
      return;
    }
    if (this.cursor && this.cursor.ownerId !== value.cursor.ownerId) {
      this.retiredOwners.add(this.cursor.ownerId);
    }
    this.cursor = value.cursor;
    if (
      this.state.error !== this.validationError ||
      JSON.stringify(this.state.account) !== JSON.stringify(value.account)
    ) {
      this.publish({ account: value.account, error: this.validationError });
    }
  }
  private current(lifecycle: number) {
    return this.running && this.lifecycle === lifecycle;
  }
  private async start(lifecycle: number) {
    try {
      await this.request("account/read", {});
      if (this.current(lifecycle)) {
        this.validationError = "";
      }
    } catch (error) {
      if (this.current(lifecycle)) {
        this.validationError = error instanceof Error ? error.message : String(error);
        this.publish({ ...this.state, error: this.validationError });
      }
    }
    if (this.current(lifecycle)) {
      startAction(this.observe(lifecycle), (actionError) => {
        this.publish({ ...this.state, error: errorMessage(actionError) });
      });
    }
  }
  private async observe(lifecycle: number) {
    try {
      while (this.current(lifecycle)) {
        // Each long poll advances one owner cursor; overlapping requests could replay observations.
        // oxlint-disable-next-line no-await-in-loop
        const value = await this.request("account/observe", { cursor: this.cursor });
        if (!this.current(lifecycle)) {
          return;
        }
        this.apply(value);
      }
    } catch (error) {
      if (!this.current(lifecycle)) {
        return;
      }
      this.publish({ ...this.state, error: String(error) });
      // Reattachment may start a replacement owner. Reads reconcile; mutations are never replayed.
      this.retry = setTimeout(() => {
        if (this.current(lifecycle)) {
          startAction(this.start(lifecycle), (actionError) => {
            this.publish({ ...this.state, error: errorMessage(actionError) });
          });
        }
      }, 2000);
    }
  }
  async reconcile() {
    const value = await this.request("account/observe", {});
    this.validationError = "";
    this.apply(value);
    return this.state.account ?? value.account;
  }
  async refresh() {
    try {
      await this.request("account/read", {});
    } catch (error) {
      this.validationError = error instanceof Error ? error.message : String(error);
      this.publish({ ...this.state, error: this.validationError });
      throw error;
    }
    return await this.reconcile();
  }
}
export const accountStore = new AccountStore();
export const AccountContext = createContext<AccountStore>(accountStore);
export function useAccountService() {
  return useContext(AccountContext);
}
export function useAccount() {
  const store = useAccountService();
  return useSyncExternalStore(store.subscribe, store.getSnapshot);
}
