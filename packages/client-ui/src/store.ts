import type { Context, Readable, State } from "@syntropika/tembra-sdk";
import { useSyncExternalStore } from "react";

export type Snapshot<T> = {
  getSnapshot: () => T;
  subscribe: (changed: () => void) => () => void;
};
export type UiStore<T> = Snapshot<T> & {
  set: (value: T) => void;
  update: (change: (current: T) => T) => void;
};
export type StateReference<T> = {
  readonly state: Readable<T>;
  update: (change: (current: Readonly<T>) => T) => Promise<unknown>;
};

/** React observes Tembra's cached value; this adapter owns no state or lifecycle. */
export function bindState<T>(
  reference: StateReference<T>,
  failed: (error: unknown) => void,
): UiStore<Readonly<T>> {
  const update = (change: (current: Readonly<T>) => T) => {
    reference.update(change).catch(failed);
  };
  return {
    getSnapshot: () => reference.state.value,
    subscribe: (changed) => reference.state.subscribe(changed),
    set: (value) => {
      update(() => value);
    },
    update,
  };
}
export function useSource<T>(source: Snapshot<T>): T {
  return useSyncExternalStore(source.subscribe, source.getSnapshot, source.getSnapshot);
}
export function selectStore<T, R>(
  store: UiStore<T>,
  read: (value: T) => R,
  write: (value: T, selected: R) => T,
): UiStore<R> {
  return {
    getSnapshot: () => read(store.getSnapshot()),
    subscribe: store.subscribe,
    set: (value) => {
      store.update((current) => write(current, value));
    },
    update: (change) => {
      store.update((current) => write(current, change(read(current))));
    },
  };
}

/** Local callbacks are admitted calls in the owner orb, never portable RPC methods. */
export function updatePresentation<T>(
  state: State<T>,
  context: Context,
  change: (current: Readonly<T>) => T,
) {
  state.update(context, change);
}
export function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
