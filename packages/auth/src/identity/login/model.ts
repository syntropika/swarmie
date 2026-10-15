import type { MergeExclusive, ReadonlyDeep } from "type-fest";

import type { EmailVerificationChallenge } from "~/src/identity/authentication/authentication-result";
import type { LoginFeedback } from "~/src/identity/shared/feedback";

/** Only verification has a challenge; other screens cannot accidentally retain one. */
export type LoginStep = MergeExclusive<
  { kind: "verify-email"; challenge: EmailVerificationChallenge },
  { kind: "sign-in" | "request-reset" | "additional-verification" }
>;

export type LoginNotice = "magic-link-sent" | "password-reset-sent";

export const loginNotices: ReadonlyDeep<Record<LoginNotice, LoginFeedback>> = {
  "magic-link-sent": {
    tone: "success",
    title: "notice.inbox",
    message: "notice.magicSent",
  },
  "password-reset-sent": {
    tone: "success",
    title: "notice.inbox",
    message: "notice.resetSent",
  },
};
