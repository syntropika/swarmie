import * as Effect from "effect/Effect";
import * as Schema from "effect/Schema";

import { RequestError } from "~/src/identity/authentication/failures";

/** A verification handle cannot be accidentally substituted with a magic-link or reset token. */
const verificationChallenge = Schema.String.pipe(
  Schema.minLength(1),
  Schema.maxLength(1024),
  Schema.brand("EmailVerificationChallenge"),
);
export type EmailVerificationChallenge = typeof verificationChallenge.Type;

const authenticationResult = Schema.Union(
  Schema.Struct({ status: Schema.Literal("authenticated") }),
  Schema.Struct({
    status: Schema.Literal("email_verification_required"),
    challenge: verificationChallenge,
  }),
  Schema.Struct({ status: Schema.Literal("additional_verification_required") }),
);

export type AuthenticationOutcome =
  | { readonly status: "authenticated" }
  | {
      readonly status: "email_verification_required";
      readonly challenge: EmailVerificationChallenge;
    }
  | { readonly status: "additional_verification_required" };

/** Decode the response before a state transition; discard validation input from public errors. */
export function decodeAuthenticationResult(value: unknown) {
  return Schema.decodeUnknown(authenticationResult)(value).pipe(
    Effect.mapError(() => new RequestError({ reason: "invalid-response", status: null })),
  );
}
