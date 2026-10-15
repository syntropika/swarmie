import { AuthPasskeyError, isForbidden } from "~/src/identity/authentication/effects";
import { toAuthFailure } from "~/src/identity/authentication/failures";
import type { PasskeyErrorCode } from "~/src/identity/passkeys/browser";
import type { LoginFeedback } from "~/src/identity/shared/feedback";
import type { PlainMessageKey } from "~/src/platform/i18n/catalogs/en";

export type PasskeyChange = "register" | { revokeId: string };
export type PasskeyChangeResult = {
  action: PasskeyChange;
  refreshed: boolean;
};

/** A refresh failure must not describe an already committed credential change as failed. */
export function passkeyChangeFeedback(result: PasskeyChangeResult): LoginFeedback {
  const registered = result.action === "register";
  let message: PlainMessageKey = "keys.refreshFailed";
  if (result.refreshed) {
    message = registered ? "keys.added" : "keys.removed";
  }
  return {
    tone: result.refreshed ? "success" : "info",
    title: registered ? "keys.addedTitle" : "keys.removedTitle",
    message,
  };
}

export function passkeyChangeError(error: unknown): LoginFeedback {
  const failure = toAuthFailure(error);
  if (isForbidden(error)) {
    return {
      tone: "info",
      title: "keys.reauthenticateTitle",
      message: "keys.reauthenticate",
    };
  }
  return {
    tone: "error",
    title: "keys.changeFailedTitle",
    message:
      failure instanceof AuthPasskeyError ? passkeyErrorMessage(failure.code) : "keys.changeFailed",
  };
}

const passkeyErrors: Record<PasskeyErrorCode, PlainMessageKey> = {
  cancelled: "error.passkeyCancelled",
  "already-registered": "error.passkeyRegistered",
  unavailable: "error.passkeyUnavailable",
  configuration: "error.passkeyConfiguration",
  failed: "error.passkeyFailed",
};

/** Translate the safe error code; adapter details never become user-facing messages. */
export function passkeyErrorMessage(code: PasskeyErrorCode): PlainMessageKey {
  return passkeyErrors[code];
}
