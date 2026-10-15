import * as Data from "effect/Data";

/** Safe transport classifications contain no response body, request data, or original cause. */
export class RequestError extends Data.TaggedError("RequestError")<{
  readonly reason: "http" | "network" | "invalid-response";
  readonly status: number | null;
}> {}

export function isUnauthorized(error: unknown): boolean {
  return error instanceof RequestError && error.status === 401;
}

export function isForbidden(error: unknown): boolean {
  return error instanceof RequestError && error.status === 403;
}
