import { describe, expect, test } from "bun:test";
import { rejects } from "node:assert/strict";

import { AccountAccessError, unavailableAccountAccess } from "~/src/account/contracts";
import { accountErrorKey, emailSchema, emailStep, oneUseTask } from "~/src/account/flow";
import { createPreviewAccess } from "~/src/account/preview-access";

describe("native account presentation boundaries", () => {
  test("only trusted lookup results choose signup or available sign-in methods", () => {
    expect(emailStep("new@example.com", { kind: "new" }).kind).toBe("signup");
    expect(
      emailStep("member@example.com", {
        kind: "existing",
        methods: { password: true, passkey: true },
      }).kind,
    ).toBe("passkey");
    expect(
      emailStep("member@example.com", {
        kind: "existing",
        methods: { password: true, passkey: false },
      }).kind,
    ).toBe("password");
    expect(() =>
      emailStep("member@example.com", {
        kind: "existing",
        methods: { password: false, passkey: false },
      }),
    ).toThrow(AccountAccessError);
    for (const email of ["", "hello", "person@", "person@example.com\nother@example.com"]) {
      expect(emailSchema.safeParse(email).success).toBe(false);
    }
  });

  test("unconfigured identity never guesses account existence or authenticates", async () => {
    const { signal } = new AbortController();
    await rejects(unavailableAccountAccess.identify("new@example.com", signal), /unavailable/u);
    await rejects(
      unavailableAccountAccess.signInWithPassword("new@example.com", "secret", signal),
      /unavailable/u,
    );
    await rejects(
      unavailableAccountAccess.signInWithPasskey("new@example.com", signal),
      /unavailable/u,
    );
    await rejects(
      unavailableAccountAccess.createAccount("new@example.com", "secret", signal),
      /unavailable/u,
    );
  });

  test("one-use operations cannot replay a credential submission, including after rejection", async () => {
    let attempts = 0;
    const task = oneUseTask(async () => {
      attempts += 1;
      throw new AccountAccessError("invalid_credentials");
    });
    await rejects(task(), /invalid_credentials/u);
    await rejects(task(), /cannot be replayed/u);
    expect(attempts).toBe(1);
  });

  test("raw service errors never become visible copy", () => {
    const privateMarker = "private-response-credential";
    expect(accountErrorKey(new Error(privateMarker))).not.toContain(privateMarker);
    expect(accountErrorKey(new AccountAccessError("cancelled"))).toContain("cancelled");
  });

  test("preview verification requires a pending challenge and consumes it once", async () => {
    const access = createPreviewAccess();
    const { signal } = new AbortController();
    const created = await access.createAccount("sam@example.com", "example-password", signal);
    if (created.kind !== "verification") {
      throw new Error("Expected verification");
    }
    await rejects(access.verifyEmail(created.challenge, "999999", signal), /invalid_code/u);
    const resent = await access.resendCode(created.challenge, signal);
    await rejects(access.verifyEmail(created.challenge, "123456", signal), /invalid_code/u);
    const verified = await access.verifyEmail(resent.challenge, "123456", signal);
    expect(verified.email).toBe("sam@example.com");
    await rejects(access.verifyEmail(resent.challenge, "123456", signal), /invalid_code/u);
  });

  test("an aborted preview operation cannot issue a result", async () => {
    const access = createPreviewAccess();
    const abort = new AbortController();
    abort.abort();
    await rejects(access.identify("alex@example.com", abort.signal));
  });
});
