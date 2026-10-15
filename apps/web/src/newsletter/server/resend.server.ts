import * as Data from "effect/Data";
import * as Effect from "effect/Effect";
import * as Either from "effect/Either";
import * as Schema from "effect/Schema";

import type { NewsletterSubscriptions } from "~/src/newsletter/server/subscribe";

/** Fetch is injected only at the external HTTP boundary for deterministic verification. */
export type NewsletterFetch = (url: string, init: RequestInit) => Promise<Response>;

/** Private provider configuration; the landing has a key separate from transactional email. */
export type ResendNewsletterOptions = {
  apiKey: string;
  segmentId?: string | undefined;
  fetcher?: NewsletterFetch;
};

type ProviderStage = "lookup" | "create" | "segment" | "membership";

/** Provider failures retain classifications, never addresses, credentials, or response bodies. */
export class NewsletterProviderError extends Data.TaggedError("NewsletterProviderError")<{
  readonly stage: ProviderStage;
  readonly reason: "network" | "timeout" | "http" | "invalid-response";
  readonly status: number | null;
}> {}

const contactSchema = Schema.Struct({
  id: Schema.NonEmptyString,
  unsubscribed: Schema.Boolean,
});

const creationSchema = Schema.Struct({ id: Schema.NonEmptyString });

const membershipSchema = Schema.Struct({ data: Schema.Array(creationSchema) });

type ResendContact = typeof contactSchema.Type;

type ProviderEffect<A> = Effect.Effect<A, NewsletterProviderError>;

/** Decode provider receipts without exposing schema issues or the original payload. */
function decodeReceipt<A, I>(
  response: Response,
  schema: Schema.Schema<A, I>,
  stage: ProviderStage,
  signal: AbortSignal,
): ProviderEffect<A> {
  return Effect.tryPromise({
    try: async (): Promise<unknown> => await response.json(),
    catch: () =>
      new NewsletterProviderError({
        stage,
        reason: signal.aborted ? "timeout" : "invalid-response",
        status: null,
      }),
  }).pipe(
    Effect.flatMap(Schema.decodeUnknown(schema)),
    // oxlint-disable-next-line promise/prefer-await-to-callbacks -- Effect.mapError transforms typed failures; it is not a Promise callback.
    Effect.mapError((error) => {
      if (error instanceof NewsletterProviderError) {
        return error;
      }
      return new NewsletterProviderError({ stage, reason: "invalid-response", status: null });
    }),
  );
}

/**
 * Store newsletter interest through Resend Contacts without calling any email endpoint.
 * Contacts observed as opted out are left untouched, and creation omits subscription
 * flags. Resend does not document duplicate-create semantics: this lookup and creation
 * cannot guarantee preservation when another writer creates a contact between them.
 * A failed create succeeds here only if a subsequent lookup finds the contact.
 */
export function createResendNewsletter(options: ResendNewsletterOptions): NewsletterSubscriptions {
  const fetcher = options.fetcher ?? fetch;

  function subscribe(email: string): ProviderEffect<void> {
    return Effect.gen(function* subscriptionWorkflow() {
      // One deadline covers the complete workflow, including confirmation after a failed write.
      const signal = AbortSignal.timeout(8000);
      const headers = {
        Authorization: `Bearer ${options.apiKey}`,
        "Content-Type": "application/json",
      };

      const request = (
        stage: ProviderStage,
        path: string,
        init: RequestInit = {},
      ): ProviderEffect<Response> =>
        Effect.tryPromise({
          try: async () =>
            await fetcher(`https://api.resend.com${path}`, {
              ...init,
              headers,
              signal,
              redirect: "error",
            }),
          catch: () =>
            new NewsletterProviderError({
              stage,
              reason: signal.aborted ? "timeout" : "network",
              status: null,
            }),
        });

      const findContact = (): ProviderEffect<ResendContact | null> =>
        Effect.gen(function* lookupContact() {
          const response = yield* request("lookup", `/contacts/${encodeURIComponent(email)}`);
          if (response.status === 404) {
            return null;
          }
          if (!response.ok) {
            return yield* httpFailure("lookup", response.status);
          }
          return yield* decodeReceipt(response, contactSchema, "lookup", signal);
        });

      const addSegment = (contact: ResendContact): ProviderEffect<void> =>
        Effect.gen(function* segmentMembership() {
          if (
            contact.unsubscribed ||
            options.segmentId === undefined ||
            options.segmentId.length === 0
          ) {
            return;
          }
          const path = `/contacts/${encodeURIComponent(contact.id)}/segments`;
          const response = yield* request(
            "segment",
            `${path}/${encodeURIComponent(options.segmentId)}`,
            { method: "POST" },
          );
          if (response.ok) {
            return;
          }

          // Confirm membership after a failed write; never repeat the mutation automatically.
          const membership = yield* request("membership", `${path}?limit=100`);
          if (!membership.ok) {
            yield* httpFailure("membership", membership.status);
            return;
          }
          const data = yield* decodeReceipt(membership, membershipSchema, "membership", signal);
          if (data.data.some((segment) => segment.id === options.segmentId)) {
            return;
          }
          yield* httpFailure("segment", response.status);
        });

      const existing = yield* findContact();
      if (existing) {
        yield* addSegment(existing);
        return;
      }

      const response = yield* request("create", "/contacts", {
        method: "POST",
        body: JSON.stringify({
          email,
          ...(options.segmentId !== undefined && options.segmentId.length > 0
            ? { segments: [{ id: options.segmentId }] }
            : {}),
        }),
      });
      if (response.ok) {
        yield* decodeReceipt(response, creationSchema, "create", signal);
        return;
      }

      // Another submission may have created the contact after the initial lookup.
      const concurrent = yield* findContact();
      if (!concurrent) {
        yield* httpFailure("create", response.status);
        return;
      }
      yield* addSegment(concurrent);
    });
  }

  return {
    async subscribe(email) {
      const result = await Effect.runPromise(Effect.either(subscribe(email)));
      if (Either.isLeft(result)) {
        throw result.left;
      }
    },
  };
}

function httpFailure(
  stage: ProviderStage,
  status: number,
): Effect.Effect<never, NewsletterProviderError> {
  return Effect.fail(new NewsletterProviderError({ stage, reason: "http", status }));
}
