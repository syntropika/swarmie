import * as Data from "effect/Data";
import * as Effect from "effect/Effect";
import * as Either from "effect/Either";
import * as Schema from "effect/Schema";

export class SubscriptionError extends Data.TaggedError("SubscriptionError")<{
  readonly reason: "invalid-email" | "rate-limit" | "unavailable" | "network" | "invalid-response";
}> {}

const accepted = Schema.Struct({ ok: Schema.Literal(true) });

type SubscriptionFetch = (input: RequestInfo | URL, init?: RequestInit) => Promise<Response>;

/** Never retry a subscription automatically or render an untrusted response message. */
export function subscribeToNewsletter(
  email: string,
  website: string,
  fetcher: SubscriptionFetch = fetch,
) {
  return Effect.gen(function* subscribe() {
    const response = yield* Effect.tryPromise({
      try: async () =>
        await fetcher("/api/subscribe", {
          method: "POST",
          headers: { "Content-Type": "application/json", Accept: "application/json" },
          body: JSON.stringify({ email, website }),
          signal: AbortSignal.timeout(15_000),
          redirect: "error",
        }),
      catch: () => new SubscriptionError({ reason: "network" }),
    });
    if (!response.ok) {
      if (response.status === 400) {
        yield* Effect.fail(new SubscriptionError({ reason: "invalid-email" }));
        return;
      }
      if (response.status === 429) {
        yield* Effect.fail(new SubscriptionError({ reason: "rate-limit" }));
        return;
      }
      yield* Effect.fail(new SubscriptionError({ reason: "unavailable" }));
      return;
    }
    const payload: unknown = yield* Effect.tryPromise({
      try: async () => await response.json(),
      catch: () => new SubscriptionError({ reason: "invalid-response" }),
    });
    yield* Schema.decodeUnknown(accepted)(payload).pipe(
      Effect.mapError(() => new SubscriptionError({ reason: "invalid-response" })),
    );
  });
}

/** Keep typed expected failures intact when crossing into React Query. */
export async function submitSubscription(email: string, website: string): Promise<void> {
  const result = await Effect.runPromise(Effect.either(subscribeToNewsletter(email, website)));
  if (Either.isLeft(result)) {
    throw result.left;
  }
}
