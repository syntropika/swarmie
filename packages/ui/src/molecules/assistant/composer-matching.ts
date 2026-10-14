/** Return the next enabled item in display order, including wraparound from a filtered selection. */
export function nextComposerItem<T extends { id: string; disabled?: boolean }>(
  items: readonly T[],
  activeId: string | undefined,
  delta: number,
): T | undefined {
  const enabled = items.filter((item) => item.disabled !== true);
  if (enabled.length === 0) {
    return undefined;
  }
  let from = enabled.findIndex((item) => item.id === activeId);
  if (from === -1) {
    from = delta > 0 ? -1 : 0;
  }
  return enabled[(from + delta + enabled.length) % enabled.length];
}
