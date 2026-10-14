// Copyright 2026 Syntropika
// SPDX-License-Identifier: Apache-2.0
import { useSyncExternalStore } from "react";
import type { AccountCursor, AccountObservation, AccountSnapshot } from "@swarmie/contracts";
import { accountRequest } from "./account";
import { nativeClient } from "./clientTransport";

type AccountState = { account?: AccountSnapshot; error: string };
/** One observation per window, shared by navigation and settings and independent of node selection. */
export class AccountStore {
  private state: AccountState = { error: "" };
  private listeners = new Set<() => void>();
  private cursor?: AccountCursor;
  private retiredOwners = new Set<string>();
  private lifecycle = 0;
  private running = false;
  private validationError = "";
  private retry?: ReturnType<typeof setTimeout>;
  getSnapshot = () => this.state;
  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    if (!this.running && nativeClient()) {
      this.running = true;
      const lifecycle = ++this.lifecycle;
      void this.start(lifecycle);
    }
    return () => {
      this.listeners.delete(listener);
      if (!this.listeners.size) {
        this.running = false;
        this.lifecycle++;
        clearTimeout(this.retry);
      }
    };
  };
  private publish(state: AccountState) {
    this.state = state;
    for (const listener of this.listeners) listener();
  }
  private apply(value: AccountObservation) {
    if (this.retiredOwners.has(value.cursor.ownerId)) return;
    if (this.cursor?.ownerId === value.cursor.ownerId && this.cursor.revision > value.cursor.revision) return;
    if (this.cursor && this.cursor.ownerId !== value.cursor.ownerId) this.retiredOwners.add(this.cursor.ownerId);
    this.cursor = value.cursor;
    if (this.state.error !== this.validationError || JSON.stringify(this.state.account) !== JSON.stringify(value.account))
      this.publish({ account: value.account, error: this.validationError });
  }
  private current(lifecycle: number) { return this.running && this.lifecycle === lifecycle; }
  private async start(lifecycle: number) {
    try { await accountRequest("account/read", {}); if (this.current(lifecycle)) this.validationError = ""; }
    catch (error) {
      if (this.current(lifecycle)) {
        this.validationError = error instanceof Error ? error.message : String(error);
        this.publish({ ...this.state, error: this.validationError });
      }
    }
    if (this.current(lifecycle)) void this.observe(lifecycle);
  }
  private async observe(lifecycle: number) {
    try {
      while (this.current(lifecycle)) {
        const value = await accountRequest("account/observe", { cursor: this.cursor });
        if (!this.current(lifecycle)) return;
        this.apply(value);
      }
    } catch (error) {
      if (!this.current(lifecycle)) return;
      this.publish({ ...this.state, error: String(error) });
      // Reattachment may start a replacement owner. Reads reconcile; mutations are never replayed.
      this.retry = setTimeout(() => { if (this.current(lifecycle)) void this.start(lifecycle); }, 2000);
    }
  }
  async reconcile() {
    const value = await accountRequest("account/observe", {});
    this.validationError = "";
    this.apply(value);
    return this.state.account ?? value.account;
  }
  async refresh() {
    try { await accountRequest("account/read", {}); }
    catch (error) {
      this.validationError = error instanceof Error ? error.message : String(error);
      this.publish({ ...this.state, error: this.validationError });
      throw error;
    }
    return this.reconcile();
  }
}
export const accountStore = new AccountStore();
export function useAccount() { return useSyncExternalStore(accountStore.subscribe, accountStore.getSnapshot); }
