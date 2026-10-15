/* oxlint-disable typescript/require-await -- In-memory preview adapters preserve promise rejection semantics without fake I/O. */
import { AccountAccessError } from "./contracts";
import type { AccountAccess, Account } from "./contracts";

function check(signal: AbortSignal) {
  signal.throwIfAborted();
}
function account(email: string): Account {
  return { id: "preview-only", email };
}

/** Only the explicit development preview route imports this in-memory presentation fixture. */
export function createPreviewAccess(): AccountAccess {
  let pendingEmail: string | undefined;
  let activeChallenge: string | undefined;
  let sequence = 0;
  function newChallenge() {
    sequence += 1;
    activeChallenge = `preview-${sequence}`;
    return activeChallenge;
  }
  return {
    identify: async (email, signal) => {
      check(signal);
      return email.toLowerCase() === "alex@example.com"
        ? { kind: "existing", methods: { password: true, passkey: true } }
        : { kind: "new" };
    },
    signInWithPassword: async (email, password, signal) => {
      check(signal);
      return password === "preview-password"
        ? await Promise.resolve({ kind: "authenticated", account: account(email) })
        : await Promise.reject(new AccountAccessError("invalid_credentials"));
    },
    signInWithPasskey: async (email, signal) => {
      check(signal);
      // A preview transition never invokes biometrics or represents an OS assertion.
      return { kind: "authenticated", account: account(email) };
    },
    createAccount: async (email, _password, signal) => {
      check(signal);
      pendingEmail = email;
      return { kind: "verification", challenge: newChallenge() };
    },
    verifyEmail: async (challenge, code, signal) => {
      check(signal);
      if (pendingEmail === undefined || challenge !== activeChallenge || code !== "123456") {
        throw new AccountAccessError("invalid_code");
      }
      const verified = account(pendingEmail);
      pendingEmail = undefined;
      activeChallenge = undefined;
      return verified;
    },
    resendCode: async (challenge, signal) => {
      check(signal);
      return challenge === activeChallenge
        ? await Promise.resolve({ challenge: newChallenge() })
        : await Promise.reject(new AccountAccessError("expired"));
    },
    addPasskey: async (signal) => {
      check(signal);
    },
    requestPasswordReset: async (_email, signal) => {
      check(signal);
    },
  };
}
