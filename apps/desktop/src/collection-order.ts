/** Sorting a copy preserves registry order and works in WebViews without Array.toSorted. */
export function sortedCopy<T>(values: readonly T[], compare: (left: T, right: T) => number): T[] {
  // The copied array is exclusively owned here; caller-owned values are never sorted in place.
  // oxlint-disable-next-line unicorn/no-array-sort
  return [...values].sort(compare);
}
