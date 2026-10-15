import { useQueryClient } from "@tanstack/react-query";
import * as Effect from "effect/Effect";
import * as Match from "effect/Match";
import { useState } from "react";

import { useAuthentication, useNavigate } from "~/src/host";
import type { AuthenticationOutcome } from "~/src/identity/authentication/authentication-result";
import { runAuthEffect } from "~/src/identity/authentication/effects";
import { authError } from "~/src/identity/authentication/errors";
import { loginNotices } from "~/src/identity/login/model";
import type { LoginNotice, LoginStep } from "~/src/identity/login/model";
import type { LoginMethod } from "~/src/identity/login/types";
import { getPasskeySupport } from "~/src/identity/passkeys/browser";
import { useAuthAction } from "~/src/identity/shared/hooks/use-auth-action";
import { clearAccountQueries } from "~/src/platform/account-cache";

/** Own form values and explicit screen transitions; Query owns pending, error, and notice state. */
export function useLogin(onAuthenticated?: () => void) {
  const { effects: authEffects } = useAuthentication();

  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const [method, setMethod] = useState<LoginMethod>("password");
  const [step, setStep] = useState<LoginStep>({ kind: "sign-in" });
  const [fields, setFields] = useState({ email: "", password: "", code: "" });

  async function complete(outcome: AuthenticationOutcome, signal: AbortSignal): Promise<void> {
    if (signal.aborted) {
      return;
    }
    setFields((current) => ({ ...current, password: "", code: "" }));
    await Match.value(outcome).pipe(
      Match.when({ status: "authenticated" }, async () => {
        await clearAccountQueries(queryClient);
        if (signal.aborted) {
          return;
        }
        if (onAuthenticated) {
          onAuthenticated();
        } else {
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

  function signIn(signal: AbortSignal) {
    return Match.value(method).pipe(
      Match.when("password", () => authEffects.signInWithPassword(fields.email, fields.password)),
      Match.when("passkey", () => authEffects.signInWithPasskey(fields.email, signal)),
      Match.when("magic", () =>
        authEffects.requestMagicLink(fields.email).pipe(Effect.as("magic-link-sent" as const)),
      ),
      Match.exhaustive,
    );
  }

  const action = useAuthAction<LoginNotice | undefined>({
    run: async (signal) => {
      const effect = Match.value(step).pipe(
        Match.when({ kind: "request-reset" }, () =>
          authEffects
            .requestPasswordReset(fields.email)
            .pipe(Effect.as("password-reset-sent" as const)),
        ),
        Match.when({ kind: "verify-email" }, (current) =>
          authEffects.verifyEmail(current.challenge, fields.code),
        ),
        Match.when({ kind: "sign-in" }, () => signIn(signal)),
        Match.when({ kind: "additional-verification" }, () => Effect.void),
        Match.exhaustive,
      );
      const outcome = await runAuthEffect(effect, { signal });
      if (signal.aborted || outcome === undefined) {
        return undefined;
      }
      if (typeof outcome === "string") {
        return outcome;
      }
      await complete(outcome, signal);
      return undefined;
    },
    onError: () => {
      setFields((current) => ({ ...current, password: "", code: "" }));
      // Every verification attempt consumes its handle, including ambiguous network failures.
      if (step.kind === "verify-email") {
        setStep({ kind: "sign-in" });
      }
    },
  });

  function restart(nextMethod: LoginMethod = method) {
    if (action.isPending) {
      return;
    }
    action.reset();
    setMethod(nextMethod);
    setFields((current) => ({ ...current, password: "", code: "" }));
    setStep({ kind: "sign-in" });
  }

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
      return authError(action.error);
    }
    if (action.data) {
      return loginNotices[action.data];
    }
    return undefined;
  }

  return {
    step,
    method,
    ...fields,
    pending: action.isPending,
    feedback: feedback(),
    passkeyAvailable: getPasskeySupport() === "available",
    restart,
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
    requestReset: () => {
      if (action.isPending) {
        return;
      }
      restart("password");
      setStep({ kind: "request-reset" });
    },
  };
}
