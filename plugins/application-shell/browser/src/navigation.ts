import type { UiStore } from "@swarmie/client-ui";

import type { Destination, ShellState } from "./index";

/** Navigation owns history; screen layout remains in its own State. */
export function createNavigation(state: UiStore<Readonly<ShellState>>) {
  return {
    navigate: (destination: Partial<Destination>): void => {
      state.update((current) => {
        const next: Destination = {
          active: current.active,
          section: current.section,
          params:
            destination.params ??
            (destination.section !== undefined && destination.section !== current.section
              ? {}
              : current.params),
          settings: current.settings,
          ...destination,
        };
        const { history } = current;
        const previous = history.entries[history.index];
        if (
          previous?.active === next.active &&
          previous.section === next.section &&
          previous.settings === next.settings &&
          JSON.stringify(previous.params) === JSON.stringify(next.params)
        ) {
          return current;
        }
        return {
          ...current,
          ...next,
          history: {
            entries: [...history.entries.slice(0, history.index + 1), next],
            index: history.index + 1,
          },
        };
      });
    },
    travel: (direction: -1 | 1): void => {
      state.update((current) => {
        const index = current.history.index + direction;
        const next = current.history.entries[index];
        return next ? { ...current, ...next, history: { ...current.history, index } } : current;
      });
    },
  };
}
