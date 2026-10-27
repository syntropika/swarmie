/** Preserve native props while omitting explicit undefined values rejected by exact optional vendor contracts. */
export function definedProps<T extends object>(
  props: T,
): { [K in keyof T]: Exclude<T[K], undefined> } {
  // SAFETY: Every retained entry has been checked at runtime. Keys and all defined values remain unchanged.
  // oxlint-disable-next-line typescript/no-unsafe-type-assertion -- The runtime filter proves the mapped exclusion of undefined; every defined value and key is retained.
  return Object.fromEntries(Object.entries(props).filter(([, value]) => value !== undefined)) as {
    [K in keyof T]: Exclude<T[K], undefined>;
  };
}
