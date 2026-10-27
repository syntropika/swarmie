import type { Snapshot } from "@swarmie/client-ui";
import type { ShellState } from "@swarmie/plugin-application-shell-browser";
import type { ViewsState } from "@swarmie/plugin-views-browser";

export type Shell = ShellState & ViewsState;

export function selectSnapshot<T, R>(
  source: Snapshot<T>,
  read: (value: T) => R,
  equal: (previous: R, next: R) => boolean = Object.is,
): Snapshot<R> {
  let previous = source.getSnapshot();
  let selected = read(previous);
  return {
    subscribe: source.subscribe,
    getSnapshot: () => {
      const current = source.getSnapshot();
      if (current !== previous) {
        previous = current;
        const next = read(current);
        if (!equal(selected, next)) {
          selected = next;
        }
      }
      return selected;
    },
  };
}

/** Reads both owners without exposing a combined write operation. */
export function layoutSnapshot(
  shell: Snapshot<Readonly<ShellState>>,
  views: Snapshot<Readonly<ViewsState>>,
): Snapshot<Shell> {
  let a = shell.getSnapshot();
  let b = views.getSnapshot();
  let value = { ...a, ...b };
  return {
    subscribe: (changed) => {
      const left = shell.subscribe(changed);
      const right = views.subscribe(changed);
      return () => {
        left();
        right();
      };
    },
    getSnapshot: () => {
      const nextA = shell.getSnapshot();
      const nextB = views.getSnapshot();
      if (a !== nextA || b !== nextB) {
        a = nextA;
        b = nextB;
        value = { ...a, ...b };
      }
      return value;
    },
  };
}
