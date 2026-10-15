import { expect, test } from "bun:test";

import * as Effect from "effect/Effect";
import * as Either from "effect/Either";

import { subscribeToNewsletter } from "~/src/newsletter/client/subscribe";

test("subscription success requires the server's confirmed outcome", async () => {
  await Promise.all(
    [null, {}, { ok: false }, { ok: "true" }].map(async (payload) => {
      const result = await Effect.runPromise(
        Effect.either(
          subscribeToNewsletter("member@example.test", "", async () => Response.json(payload)),
        ),
      );
      expect(Either.isLeft(result)).toBe(true);
      if (Either.isLeft(result)) {
        expect(result.left.reason).toBe("invalid-response");
      }
    }),
  );
  const accepted = await Effect.runPromise(
    Effect.either(
      subscribeToNewsletter("member@example.test", "", async () => Response.json({ ok: true })),
    ),
  );
  expect(Either.isRight(accepted)).toBe(true);
});

test("failed subscriptions are classified without leaking provider text or retrying", async () => {
  await Promise.all(
    (
      [
        [400, "invalid-email"],
        [429, "rate-limit"],
        [503, "unavailable"],
      ] as const
    ).map(async ([status, reason]) => {
      let attempts = 0;
      const result = await Effect.runPromise(
        Effect.either(
          subscribeToNewsletter("member@example.test", "", async () => {
            attempts += 1;
            return Response.json({ error: "private-provider-detail" }, { status });
          }),
        ),
      );
      expect(attempts).toBe(1);
      expect(Either.isLeft(result)).toBe(true);
      if (Either.isLeft(result)) {
        expect(result.left.reason).toBe(reason);
        expect(JSON.stringify(result.left)).not.toContain("private-provider-detail");
      }
    }),
  );
});

test("network failures retain no arbitrary error data and do not retry", async () => {
  let attempts = 0;
  const result = await Effect.runPromise(
    Effect.either(
      subscribeToNewsletter("member@example.test", "", async () => {
        attempts += 1;
        throw new Error("private-network-detail");
      }),
    ),
  );
  expect(attempts).toBe(1);
  expect(Either.isLeft(result)).toBe(true);
  if (Either.isLeft(result)) {
    expect(result.left.reason).toBe("network");
    expect(JSON.stringify(result.left)).not.toContain("private-network-detail");
  }
});

test("the request preserves the honeypot and rejects redirects", async () => {
  await Effect.runPromise(
    subscribeToNewsletter("member@example.test", "honeypot", async (input, init) => {
      expect(input).toBe("/api/subscribe");
      expect(init?.method).toBe("POST");
      expect(init?.redirect).toBe("error");
      expect(JSON.parse(typeof init?.body === "string" ? init.body : "null")).toEqual({
        email: "member@example.test",
        website: "honeypot",
      });
      return Response.json({ ok: true });
    }),
  );
});
