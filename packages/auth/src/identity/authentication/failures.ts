import { RequestError } from "@swarmie/api";
import * as Data from "effect/Data";

import { PasskeyError } from "~/src/identity/passkeys/browser";
import type { PasskeyErrorCode } from "~/src/identity/passkeys/browser";

export { RequestError, isUnauthorized, isForbidden } from "@swarmie/api";

/** Messages originate in the browser adapter's fixed copy, never from a provider response. */
export class AuthPasskeyError extends Data.TaggedError("AuthPasskeyError")<{
  readonly code: PasskeyErrorCode;
  readonly message: string;
}> {}

export type AuthFailure = RequestError | AuthPasskeyError;

export function toAuthFailure(error: unknown): AuthFailure {
  if (error instanceof RequestError || error instanceof AuthPasskeyError) {
    return error;
  }
  if (error instanceof PasskeyError) {
    return new AuthPasskeyError({ code: error.code, message: error.message });
  }
  if (error instanceof DOMException && error.name === "AbortError") {
    return new AuthPasskeyError({ code: "cancelled", message: "The request was cancelled." });
  }
  return new RequestError({ reason: "network", status: null });
}
