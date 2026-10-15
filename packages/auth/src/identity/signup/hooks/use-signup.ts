import { useQueryClient } from "@tanstack/react-query";
import * as Effect from "effect/Effect";
import * as Match from "effect/Match";
import { useState } from "react";

import { useAuthentication, useNavigate } from "~/src/host";
import type { AuthenticationOutcome } from "~/src/identity/authentication/authentication-result";
import { runAuthEffect } from "~/src/identity/authentication/effects";
import { authError } from "~/src/identity/authentication/errors";
import { useAuthAction } from "~/src/identity/shared/hooks/use-auth-action";
import { magicLinkSent } from "~/src/identity/signup/model";
import type { SignupMethod, SignupStep } from "~/src/identity/signup/model";
import { clearAccountQueries } from "~/src/platform/account-cache";
import { observeBackgroundTask } from "~/src/platform/background-task";

/** Credentials and verification handles stay outside mutation variables, results, and caches. */
export function useSignup() {
  const { effects: authEffects } = useAuthentication();

  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const [method, setMethod] = useState<SignupMethod>("password");
  const [step, setStep] = useState<SignupStep>({ kind: "sign-up" });
  const [fields, setFields] = useState({ email: "", password: "", code: "" });

  async function complete(outcome: AuthenticationOutcome, signal: AbortSignal) {
    if (signal.aborted) {
      return;
    }
    await Match.value(outcome).pipe(
      Match.when({ status: "authenticated" }, async () => {
        await clearAccountQueries(queryClient);
        if (!signal.aborted) {
          await navigate({ to: "/", replace: true });
        }
      }),
      Match.when({ status: "email_verification_required" }, (result) => {
        setStep({ kind: "verify-email", challenge: result.challenge });
      }),
      Match.when({ status: "additional_verification_required" }, () => {
        setStep({ kind: "additional-verification" });
      }),
      Match.exhaustive,
    );
  }

  const action = useAuthAction<"magic-link-sent" | undefined>({
    run: async (signal) => {
      const effect = Match.value(step).pipe(
        Match.when({ kind: "sign-up" }, () =>
          Match.value(method).pipe(
            Match.when("password", () =>
              authEffects.signUpWithPassword(fields.email, fields.password),
            ),
            Match.when("magic", () =>
              authEffects
                .requestMagicLink(fields.email)
                .pipe(Effect.as("magic-link-sent" as const)),
            ),
            Match.exhaustive,
          ),
        ),
        Match.when({ kind: "verify-email" }, (current) =>
          authEffects.verifyEmail(current.challenge, fields.code),
        ),
        Match.when({ kind: "verification-recovery" }, () => Effect.void),
        Match.when({ kind: "additional-verification" }, () => Effect.void),
        Match.exhaustive,
      );
      try {
        const outcome = await runAuthEffect(effect, { signal });
        if (signal.aborted || outcome === undefined) {
          return undefined;
        }
        if (outcome === "magic-link-sent") {
          return outcome;
        }
        await complete(outcome, signal);
        return undefined;
      } finally {
        if (!signal.aborted) {
          setFields((current) => ({ ...current, password: "", code: "" }));
        }
      }
    },
    onError: () => {
      // Verification handles are consumed even if the result was lost in transit.
      if (step.kind === "verify-email") {
        setStep({ kind: "verification-recovery" });
      }
    },
  });

  function changeField(field: keyof typeof fields, value: string) {
    setFields((current) => ({ ...current, [field]: value }));
    if (!action.isPending) {
      action.reset();
    }
  }

  function feedback() {
    if (action.isPending) {
      return undefined;
    }
    if (action.error) {
      return authError(action.error, step.kind === "sign-up" ? "sign-up" : "sign-in");
    }
    if (action.data === "magic-link-sent") {
      return magicLinkSent;
    }
    return undefined;
  }

  return {
    method,
    step,
    ...fields,
    pending: action.isPending,
    feedback: feedback(),
    submit: action.execute,
    changeEmail: (value: string) => {
      changeField("email", value);
    },
    changePassword: (value: string) => {
      changeField("password", value);
    },
    changeCode: (value: string) => {
      changeField("code", value);
    },
    changeMethod: (nextMethod: SignupMethod) => {
      if (action.isPending || step.kind !== "sign-up") {
        return;
      }
      action.reset();
      setMethod(nextMethod);
      setFields((current) => ({ ...current, password: "", code: "" }));
    },
    signIn: () => {
      observeBackgroundTask(navigate({ to: "/login" }));
    },
  };
}
