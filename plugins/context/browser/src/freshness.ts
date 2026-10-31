import type { MetadataFreshness } from "./types";

/** Validate exact wire timestamps without rounding through a JSON number. */
function timestamp(value: unknown): string {
  if (
    typeof value !== "string" ||
    value.length === 0 ||
    value.length > 20 ||
    /[^0-9]/u.test(value) ||
    (value.length > 1 && value.startsWith("0")) ||
    BigInt(value) > 18_446_744_073_709_551_615n
  ) {
    throw new Error("The node returned an invalid metadata timestamp.");
  }
  return value;
}

function isObject(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

export function metadataFreshness(value: unknown): MetadataFreshness {
  if (!isObject(value)) {
    throw new Error("The node returned invalid metadata freshness.");
  }
  const raw = value;
  const preparedAt = timestamp(raw.prepared_at_ms);
  const validUntil = raw.valid_until_ms === null ? null : timestamp(raw.valid_until_ms);
  const stale = raw.stale_limit_evidence;
  if (
    (validUntil !== null && BigInt(validUntil) <= BigInt(preparedAt)) ||
    typeof stale !== "number" ||
    !Number.isInteger(stale) ||
    stale < 0 ||
    stale > 4_294_967_295
  ) {
    throw new Error("The node returned invalid metadata freshness.");
  }
  return {
    prepared_at_ms: preparedAt,
    valid_until_ms: validUntil,
    stale_limit_evidence: stale,
  };
}

/** This formatting is presentation only; it grants no metadata admission. */
export function metadataTime(value: string): string {
  const milliseconds = BigInt(timestamp(value));
  if (milliseconds > 8_640_000_000_000_000n) {
    return "Outside the calendar range";
  }
  return new Date(Number(milliseconds)).toISOString();
}
