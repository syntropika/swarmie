export function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

export function record(value: unknown): Record<string, unknown> {
  if (!isRecord(value)) {
    throw new TypeError("Expected a JSON object.");
  }
  return value;
}

export function responseResult(value: unknown): unknown {
  const envelope = record(value);
  if (envelope.error !== undefined && envelope.error !== null) {
    const error = record(envelope.error);
    throw new Error(typeof error.message === "string" ? error.message : "The request failed.");
  }
  if (!Object.hasOwn(envelope, "result")) {
    throw new TypeError("Missing response result.");
  }
  return envelope.result;
}
