import { z } from "zod";

import { AccountAccessError } from "./contracts";
import type { Account, EmailLookup, SignInOptions } from "./contracts";

export type AccountStep =
  | { kind: "email"; email: string }
  | { kind: "passkey" | "password"; email: string; methods: SignInOptions }
  | { kind: "signup"; email: string }
  | { kind: "verify"; email: string; challenge: string; purpose: "signup" | "signin" }
  | { kind: "add-passkey" | "complete"; account: Account }
  | { kind: "reset" | "reset-sent"; email: string };

export const emailSchema = z.email().max(254);

export function emailStep(email: string, lookup: EmailLookup): AccountStep {
  if (lookup.kind === "new") {
    return { kind: "signup", email };
  }
  if (!lookup.methods.passkey && !lookup.methods.password) {
    throw new AccountAccessError("unavailable");
  }
  return { kind: lookup.methods.passkey ? "passkey" : "password", email, methods: lookup.methods };
}

export function stepEmail(step: AccountStep): string {
  return "email" in step ? step.email : step.account.email;
}

export function accountErrorMessage(error: unknown): string {
  if (!(error instanceof AccountAccessError)) {
    return "We couldn't connect. Check your connection and try again.";
  }
  const { reason } = error;
  switch (reason) {
    case "unavailable": {
      return "Account sign-in is not available yet. Please try again later.";
    }
    case "invalid_credentials": {
      return "That password didn't work. Try again or reset your password.";
    }
    case "invalid_code": {
      return "That code didn't work. Check the email and try again.";
    }
    case "expired": {
      return "That code has expired. Request a new code to continue.";
    }
    case "cancelled": {
      return "Passkey sign-in was cancelled. Try again or use your password.";
    }
    case "rate_limited": {
      return "Too many attempts. Please wait before trying again.";
    }
    default: {
      const exhaustive: never = reason;
      return exhaustive;
    }
  }
}

/** A cached mutation descriptor releases its credential-bearing closure as soon as execution starts. */
export function oneUseTask<T>(task: () => Promise<T>): () => Promise<T> {
  let pending: (() => Promise<T>) | undefined = task;
  return async () => {
    const execute = pending;
    pending = undefined;
    if (!execute) {
      throw new Error("Account operations cannot be replayed");
    }
    return await execute();
  };
}
