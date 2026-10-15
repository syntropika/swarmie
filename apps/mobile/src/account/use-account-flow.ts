import { useMutation } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";

import { AccountAccessError } from "./contracts";
import type { AccountAccess, AuthenticationResult } from "./contracts";
import { accountErrorMessage, emailStep, oneUseTask, stepEmail } from "./flow";
import type { AccountStep } from "./flow";

type Outcome = { step: AccountStep; notice?: string };
type Job = { revision: number; execute: () => Promise<Outcome> };

function authenticated(
  result: AuthenticationResult,
  purpose: "signup" | "signin",
  email: string,
): Outcome {
  if (result.kind === "verification") {
    return { step: { kind: "verify", email, challenge: result.challenge, purpose } };
  }
  return {
    step: { kind: purpose === "signup" ? "add-passkey" : "complete", account: result.account },
  };
}

export function useAccountFlow(access: AccountAccess) {
  const [step, setStep] = useState<AccountStep>({ kind: "email", email: "" });
  const [notice, setNotice] = useState<string>();
  const revision = useRef(0);
  const active = useRef<AbortController | null>(null);
  const busy = useRef(false);
  const mutation = useMutation({
    mutationFn: async (job: Job) => {
      try {
        const outcome = await job.execute();
        if (job.revision === revision.current) {
          setStep(outcome.step);
          setNotice(outcome.notice);
        }
      } catch (error) {
        // Cache only bounded failures; challenge handles stay in their owning flow state.
        throw error instanceof AccountAccessError ? error : new AccountAccessError("unavailable");
      }
    },
    retry: false,
    gcTime: 0,
    onSettled: (_result, _error, job) => {
      if (job.revision === revision.current) {
        busy.current = false;
        active.current = null;
      }
    },
  });

  useEffect(
    () => () => {
      revision.current += 1;
      active.current?.abort();
    },
    [],
  );

  function navigate(next: AccountStep) {
    revision.current += 1;
    active.current?.abort();
    active.current = null;
    busy.current = false;
    mutation.reset();
    setNotice(undefined);
    setStep(next);
  }

  function run(task: (signal: AbortSignal) => Promise<Outcome>) {
    if (busy.current) {
      return;
    }
    busy.current = true;
    const controller = new AbortController();
    active.current = controller;
    setNotice(undefined);
    mutation.mutate({
      revision: revision.current,
      execute: oneUseTask(async () => await task(controller.signal)),
    });
  }

  return {
    step,
    pending: mutation.isPending,
    error: mutation.isError ? accountErrorMessage(mutation.error) : undefined,
    notice,
    navigate,
    changeEmail: () => {
      navigate({ kind: "email", email: stepEmail(step) });
    },
    identify: (email: string) => {
      run(async (signal) => ({ step: emailStep(email, await access.identify(email, signal)) }));
    },
    password: (password: string) => {
      if (step.kind !== "password" && step.kind !== "signup") {
        return;
      }
      const purpose = step.kind === "signup" ? "signup" : "signin";
      run(async (signal) =>
        authenticated(
          await (purpose === "signup"
            ? access.createAccount(step.email, password, signal)
            : access.signInWithPassword(step.email, password, signal)),
          purpose,
          step.email,
        ),
      );
    },
    passkey: () => {
      if (step.kind !== "passkey" && step.kind !== "password") {
        return;
      }
      run(async (signal) =>
        authenticated(await access.signInWithPasskey(step.email, signal), "signin", step.email),
      );
    },
    verify: (code: string) => {
      if (step.kind !== "verify") {
        return;
      }
      run(async (signal) => ({
        step: {
          kind: step.purpose === "signup" ? "add-passkey" : "complete",
          account: await access.verifyEmail(step.challenge, code, signal),
        },
      }));
    },
    resend: () => {
      if (step.kind !== "verify") {
        return;
      }
      run(async (signal) => {
        const result = await access.resendCode(step.challenge, signal);
        return {
          step: { ...step, challenge: result.challenge },
          notice: "A new code has been sent.",
        };
      });
    },
    addPasskey: () => {
      if (step.kind !== "add-passkey") {
        return;
      }
      run(async (signal) => {
        await access.addPasskey(signal);
        return { step: { kind: "complete", account: step.account } };
      });
    },
    requestReset: () => {
      if (step.kind !== "reset") {
        return;
      }
      run(async (signal) => {
        await access.requestPasswordReset(step.email, signal);
        return { step: { kind: "reset-sent", email: step.email } };
      });
    },
  };
}

export type AccountFlow = ReturnType<typeof useAccountFlow>;
