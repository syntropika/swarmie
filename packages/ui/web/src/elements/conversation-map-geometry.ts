// SPDX-License-Identifier: MIT
// Copyright (c) 2026 OpenClaw Foundation
// Source revision and license: ../../vendor/openclaw-position-rail/NOTICE.md.

export const POSITION_RAIL_MARKER_HEIGHT = 12;
const POSITION_RAIL_MARKER_OVERSCAN = 6;

export function positionRailWindowIndexes(
  count: number,
  markerIndexes: ReadonlyMap<string, number>,
  offset: number,
  height: number,
  ...retainedIds: readonly (string | null | undefined)[]
): number[] {
  const start = Math.max(
    0,
    Math.floor(offset / POSITION_RAIL_MARKER_HEIGHT) - POSITION_RAIL_MARKER_OVERSCAN,
  );
  const end = Math.min(
    count,
    Math.ceil((offset + height) / POSITION_RAIL_MARKER_HEIGHT) + POSITION_RAIL_MARKER_OVERSCAN,
  );
  const indexes = new Set<number>();
  for (let index = start; index < end; index += 1) {
    indexes.add(index);
  }
  // Roving Tab entry and an explored focus survive an independently scrolled rail.
  for (const id of retainedIds) {
    const index = id !== null && id !== undefined ? markerIndexes.get(id) : undefined;
    if (index !== undefined) {
      indexes.add(index);
    }
  }
  return [...indexes].toSorted((left, right) => left - right);
}
