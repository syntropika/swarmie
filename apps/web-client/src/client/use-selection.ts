import { useSource } from "@swarmie/client-ui";
import type { Snapshot } from "@swarmie/client-ui";
import { useMemo } from "react";

import { selectSnapshot } from "./projections";

/** Keep selectors stable; projections observe the owner without copying its state. */
export function useSelection<T, R>(
  source: Snapshot<T>,
  read: (value: T) => R,
  equal: (previous: R, next: R) => boolean = Object.is,
): R {
  const selection = useMemo(() => selectSnapshot(source, read, equal), [source, read, equal]);
  return useSource(selection);
}
