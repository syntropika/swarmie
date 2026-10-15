export function hasText(value: string | null | undefined): value is string {
  return value !== undefined && value !== null && value.length > 0;
}

export function hasCount(value: number | null | undefined): value is number {
  return value !== undefined && value !== null && value !== 0 && !Number.isNaN(value);
}

export function scalarLabel(value: unknown, fallback: string): string {
  if (typeof value === "string") {
    return value;
  }
  if (typeof value === "number" || typeof value === "boolean") {
    return String(value);
  }
  return fallback;
}
