import { z } from "zod";

import { AccountAccessError } from "~/src/account/contracts";
import type { Account, EmailLookup, SignInOptions } from "~/src/account/contracts";
import type { MessageKey } from "~/src/platform/i18n/catalogs/en";

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

export function accountErrorKey(error: unknown): Extract<MessageKey, `error.${string}`> {
  if (!(error instanceof AccountAccessError)) {
    return "error.connection";
  }
  const { reason } = error;
  switch (reason) {
    case "unavailable": {
      return "error.unavailable";
    }
    case "invalid_credentials": {
      return "error.invalidCredentials";
    }
    case "invalid_code": {
      return "error.invalidCode";
    }
    case "expired": {
      return "error.expired";
    }
    case "cancelled": {
      return "error.cancelled";
    }
    case "rate_limited": {
      return "error.rateLimited";
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
