export type Account = { id: string; email: string };
export type SignInOptions = { password: boolean; passkey: boolean };
export type EmailLookup = { kind: "new" } | { kind: "existing"; methods: SignInOptions };
export type AuthenticationResult =
  | { kind: "authenticated"; account: Account }
  | { kind: "verification"; challenge: string };

/** Client presentation port. Hosted identity and native credential ceremonies supply the implementation. */
export type AccountAccess = {
  identify: (email: string, signal: AbortSignal) => Promise<EmailLookup>;
  signInWithPassword: (
    email: string,
    password: string,
    signal: AbortSignal,
  ) => Promise<AuthenticationResult>;
  signInWithPasskey: (email: string, signal: AbortSignal) => Promise<AuthenticationResult>;
  createAccount: (
    email: string,
    password: string,
    signal: AbortSignal,
  ) => Promise<AuthenticationResult>;
  verifyEmail: (challenge: string, code: string, signal: AbortSignal) => Promise<Account>;
  resendCode: (challenge: string, signal: AbortSignal) => Promise<{ challenge: string }>;
  addPasskey: (signal: AbortSignal) => Promise<void>;
  requestPasswordReset: (email: string, signal: AbortSignal) => Promise<void>;
};

export class AccountAccessError extends Error {
  constructor(
    public readonly reason:
      | "unavailable"
      | "invalid_credentials"
      | "invalid_code"
      | "expired"
      | "cancelled"
      | "rate_limited",
  ) {
    super(reason);
    this.name = "AccountAccessError";
  }
}

/** Never infer account existence or mint a session in the client when identity is unavailable. */
// oxlint-disable-next-line typescript/promise-function-async -- An unavailable adapter rejects without scheduling synthetic async work.
function unavailable(): Promise<never> {
  return Promise.reject(new AccountAccessError("unavailable"));
}

export const unavailableAccountAccess: AccountAccess = {
  identify: unavailable,
  signInWithPassword: unavailable,
  signInWithPasskey: unavailable,
  createAccount: unavailable,
  verifyEmail: unavailable,
  resendCode: unavailable,
  addPasskey: unavailable,
  requestPasswordReset: unavailable,
};
