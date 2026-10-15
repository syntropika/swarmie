import * as Match from "effect/Match";

import { toAuthFailure } from "~/src/identity/authentication/failures";
import { passkeyErrorMessage } from "~/src/identity/passkeys/feedback";
import type { LoginFeedback } from "~/src/identity/shared/feedback";
import type { PlainMessageKey } from "~/src/platform/i18n/catalogs/en";

const rejectedCopy: Record<
  "sign-in" | "sign-up" | "reset-password",
  { title: PlainMessageKey; message: PlainMessageKey }
> = {
  "sign-in": {
    title: "error.signInTitle",
    message: "error.signIn",
  },
  "sign-up": {
    title: "error.signUpTitle",
    message: "error.signUp",
  },
  "reset-password": {
    title: "error.resetTitle",
    message: "error.reset",
  },
};

/** Map typed failures to fixed copy; never render a provider response or unexpected exception. */
export function authError(
  error: unknown,
  operation: "sign-in" | "sign-up" | "reset-password" = "sign-in",
): LoginFeedback {
  return Match.value(toAuthFailure(error)).pipe(
    Match.tag("AuthPasskeyError", (failure): LoginFeedback => ({
      tone: "info" as const,
      title: "error.passkeyTitle",
      message: passkeyErrorMessage(failure.code),
    })),
    Match.tag("RequestError", (failure): LoginFeedback => {
      if (failure.status === 429) {
        return {
          tone: "error" as const,
          title: "error.waitTitle",
          message: "error.rateLimit",
        };
      }
      if (
        failure.status !== null &&
        ([400, 401, 403, 422].includes(failure.status) ||
          (operation === "sign-up" && failure.status === 409))
      ) {
        return { tone: "error" as const, ...rejectedCopy[operation] };
      }
      return {
        tone: "error" as const,
        title: "error.connectTitle",
        message: "error.connect",
      };
    }),
    Match.exhaustive,
  );
}
