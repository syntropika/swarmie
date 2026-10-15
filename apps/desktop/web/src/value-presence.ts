/** Empty optional text represents an absent user choice or provider field. */
export function hasText(value: string | null | undefined): value is string {
  return value !== undefined && value !== null && value.length > 0;
}

export function nonEmptyText(value: string | null | undefined): string | undefined {
  return hasText(value) ? value : undefined;
}

/** Numeric presence preserves zero/NaN handling without converting a stored value. */
export function hasNumber(value: number | null | undefined): value is number {
  return value !== undefined && value !== null && value !== 0 && !Number.isNaN(value);
}
