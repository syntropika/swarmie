import { useMutation, useQueryClient } from "@tanstack/react-query";
import type * as Effect from "effect/Effect";
import { useEffect, useRef, useState } from "react";

import { useAuthentication, useNavigate } from "~/src/host";
import type {
  AuthenticationOutcome,
  EmailVerificationChallenge,
} from "~/src/identity/authentication/authentication-result";
import { RequestError, runAuthEffect } from "~/src/identity/authentication/effects";
import type { AuthFailure } from "~/src/identity/authentication/effects";
import { takeMagicLinkToken } from "~/src/identity/magic-link/token";
import type { LoginFeedback } from "~/src/identity/shared/feedback";
import { clearAccountQueries } from "~/src/platform/account-cache";
import { observeBackgroundTask } from "~/src/platform/background-task";

type Step =
  | { kind: "loading" | "consuming" | "additional-verification" }
  | { kind: "unavailable"; feedback?: LoginFeedback | undefined }
  | { kind: "ready"; token: string }
  | { kind: "verify-email"; challenge: EmailVerificationChallenge };

type Operation = {
  generation: number;
  controller: AbortController;
  effect: Effect.Effect<AuthenticationOutcome, AuthFailure>;
};

function failureFeedback(error: unknown): LoginFeedback | undefined {
  if (error instanceof RequestError && error.status === 429) {
    return {
      tone: "error",
      title: "error.waitTitle",
      message: "magic.rateLimit",
    };
  }
  if (!(error instanceof RequestError) || error.status === null || error.status >= 500) {
    return {
      tone: "error",
      title: "magic.confirmFailedTitle",
      message: "magic.confirmFailed",
    };
  }
  return undefined;
}

/** Tokens stay in memory; only a deliberate action can exchange a one-shot proof. */
export function useMagicLink() {
  const { effects: authEffects } = useAuthentication();

  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const [step, setStep] = useState<Step>({ kind: "loading" });
  const [code, setCode] = useState("");
  const mounted = useRef(false);
  const capturedInitialFragment = useRef(false);
  const generation = useRef(0);
  const operation = useRef<Operation | null>(null);

  useEffect(() => {
    mounted.current = true;
    function captureFragment() {
      const token = takeMagicLinkToken(window.location, window.history);
      generation.current += 1;
      operation.current?.controller.abort();
      setCode("");
      setStep(token === null ? { kind: "unavailable" } : { kind: "ready", token });
    }
    if (!capturedInitialFragment.current) {
      capturedInitialFragment.current = true;
      captureFragment();
    }
    window.addEventListener("hashchange", captureFragment);
    return () => {
      mounted.current = false;
      generation.current += 1;
      operation.current?.controller.abort();
      window.removeEventListener("hashchange", captureFragment);
    };
  }, []);

  function isCurrent(request: Operation): boolean {
    return mounted.current && request.generation === generation.current;
  }

  async function verified(result: AuthenticationOutcome, request: Operation) {
    if (!isCurrent(request)) {
      return;
    }
    setCode("");
    // oxlint-disable-next-line default-case -- The union is checked by typescript/switch-exhaustiveness-check.
    switch (result.status) {
      case "authenticated": {
        await clearAccountQueries(queryClient);
        if (!isCurrent(request)) {
          return;
        }
        await navigate({ to: "/", replace: true });
        return;
      }
      case "email_verification_required": {
        setStep({ kind: "verify-email", challenge: result.challenge });
        return;
      }
      case "additional_verification_required": {
        setStep({ kind: "additional-verification" });
      }
    }
  }

  const mutation = useMutation({
    // Neither variables nor mutation data retain tokens, challenges, or verification codes.
    mutationFn: async () => {
      const request = operation.current;
      if (!request) {
        return;
      }
      await verified(
        await runAuthEffect(request.effect, { signal: request.controller.signal }),
        request,
      );
    },
    retry: false,
    networkMode: "always",
    gcTime: 0,
  });

  async function submit() {
    if (
      !mounted.current ||
      operation.current ||
      (step.kind !== "ready" && step.kind !== "verify-email")
    ) {
      return;
    }
    const effect =
      step.kind === "ready"
        ? authEffects.verifyMagicLink(step.token)
        : authEffects.verifyEmail(step.challenge, code);
    const request = { generation: generation.current, effect, controller: new AbortController() };
    operation.current = request;
    setCode("");
    setStep({ kind: "consuming" });
    try {
      await mutation.mutateAsync();
    } catch (error) {
      if (isCurrent(request)) {
        setStep({ kind: "unavailable", feedback: failureFeedback(error) });
      }
    } finally {
      if (operation.current === request) {
        operation.current = null;
      }
    }
  }

  function backToSignIn() {
    generation.current += 1;
    operation.current?.controller.abort();
    setCode("");
    setStep({ kind: "unavailable" });
    observeBackgroundTask(navigate({ to: "/login", replace: true }));
  }

  return {
    stage: step.kind,
    pending: mutation.isPending,
    feedback: step.kind === "unavailable" ? step.feedback : undefined,
    code,
    setCode,
    submit: () => {
      observeBackgroundTask(submit());
    },
    backToSignIn,
  };
}
