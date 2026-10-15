import type { RequestError, SwarmieClient } from "@swarmie/api";
import * as Effect from "effect/Effect";

import { decodeAuthenticationResult } from "~/src/identity/authentication/authentication-result";
import type { EmailVerificationChallenge } from "~/src/identity/authentication/authentication-result";
import {
  AuthPasskeyError,
  isUnauthorized,
  toAuthFailure,
} from "~/src/identity/authentication/failures";
import type { AuthFailure } from "~/src/identity/authentication/failures";
import { authenticatePasskey, createPasskey } from "~/src/identity/passkeys/browser";

export { runRequestEffect as runAuthEffect } from "~/src/platform/api/run-effect";
export {
  RequestError,
  AuthPasskeyError,
  isUnauthorized,
  isForbidden,
} from "~/src/identity/authentication/failures";

export type { AuthFailure } from "~/src/identity/authentication/failures";

type AuthClient = Pick<
  SwarmieClient,
  | "me"
  | "logout"
  | "passkeys"
  | "revokePasskey"
  | "signInWithPassword"
  | "signUpWithPassword"
  | "verifyEmail"
  | "requestMagicLink"
  | "verifyMagicLink"
  | "requestPasswordReset"
  | "resetPassword"
  | "passkeySignInOptions"
  | "verifyPasskeySignIn"
  | "passkeyRegistrationOptions"
  | "verifyPasskeyRegistration"
>;

type PasskeyDevice = {
  authenticate: typeof authenticatePasskey;
  create: typeof createPasskey;
};

function requestDevice<A>(
  operation: (signal: AbortSignal) => Promise<A>,
): Effect.Effect<A, AuthFailure> {
  return Effect.tryPromise({ try: operation, catch: toAuthFailure });
}

function checkCancellation(signal?: AbortSignal) {
  return Effect.suspend(() => {
    if (signal?.aborted === true) {
      return Effect.fail(
        new AuthPasskeyError({
          code: "cancelled",
          message: "The passkey request was cancelled. You can try again.",
        }),
      );
    }
    return Effect.void;
  });
}

const defaultPasskeyDevice: PasskeyDevice = {
  authenticate: authenticatePasskey,
  create: createPasskey,
};

function authenticate(operation: Effect.Effect<unknown, RequestError>) {
  return operation.pipe(Effect.flatMap(decodeAuthenticationResult));
}

/** Authentication workflows have typed failures, one-shot proofs, and no automatic retries. */
export function createAuthEffects(
  client: AuthClient,
  device: PasskeyDevice = defaultPasskeyDevice,
) {
  return {
    session: () => client.me().pipe(Effect.map((session) => session.user)),
    passkeys: () => client.passkeys(),
    revokePasskey: (id: string) => client.revokePasskey(id),
    signInWithPassword: (email: string, password: string) =>
      authenticate(client.signInWithPassword(email.trim(), password)),
    signUpWithPassword: (email: string, password: string) =>
      authenticate(client.signUpWithPassword(email.trim(), password)),
    verifyEmail: (challenge: EmailVerificationChallenge, code: string) =>
      authenticate(client.verifyEmail(challenge, code)),
    requestMagicLink: (email: string) => client.requestMagicLink(email.trim()),
    verifyMagicLink: (token: string) => authenticate(client.verifyMagicLink(token)),
    requestPasswordReset: (email: string) => client.requestPasswordReset(email.trim()),
    resetPassword: (token: string, password: string) => client.resetPassword(token, password),
    signInWithPasskey: (email: string, signal?: AbortSignal) =>
      Effect.gen(function* signInWithPasskey() {
        yield* checkCancellation(signal);
        const challenge = yield* client.passkeySignInOptions(email.trim());
        yield* checkCancellation(signal);
        const credential = yield* requestDevice(
          async (effectSignal) =>
            await device.authenticate(
              challenge.public_key,
              signal ? AbortSignal.any([signal, effectSignal]) : effectSignal,
            ),
        );
        yield* checkCancellation(signal);
        return yield* authenticate(client.verifyPasskeySignIn(challenge.challenge, credential));
      }),
    registerPasskey: (name: string, signal?: AbortSignal) =>
      Effect.gen(function* registerPasskey() {
        yield* checkCancellation(signal);
        const challenge = yield* client.passkeyRegistrationOptions(name.trim());
        yield* checkCancellation(signal);
        const credential = yield* requestDevice(
          async (effectSignal) =>
            await device.create(
              challenge.public_key,
              signal ? AbortSignal.any([signal, effectSignal]) : effectSignal,
            ),
        );
        yield* checkCancellation(signal);
        yield* client.verifyPasskeyRegistration(challenge.challenge, credential);
      }),
    logout: () =>
      client.logout().pipe(
        Effect.catchAll((logoutError) =>
          client.me().pipe(
            Effect.matchEffect({
              onFailure: (sessionError) =>
                isUnauthorized(sessionError) ? Effect.void : Effect.fail(sessionError),
              onSuccess: () => Effect.fail(logoutError),
            }),
          ),
        ),
      ),
  };
}

export type AuthEffects = ReturnType<typeof createAuthEffects>;
