import type { EmailVerificationChallenge } from "~/src/identity/authentication/authentication-result";
import type { AuthenticationMethod } from "~/src/identity/shared/authentication-method";
import type { LoginFeedback } from "~/src/identity/shared/feedback";

/** A passkey is registered only after email ownership has been verified. */
export type SignupMethod = Exclude<AuthenticationMethod, "passkey">;

/** Failed verification never falls back to submitting account creation again. */
export type SignupStep =
  | { kind: "sign-up" }
  | { kind: "verify-email"; challenge: EmailVerificationChallenge }
  | { kind: "verification-recovery" }
  | { kind: "additional-verification" };

export const magicLinkSent: LoginFeedback = {
  tone: "success",
  title: "notice.inbox",
  message: "notice.magicSent",
};
