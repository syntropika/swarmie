import { describe, expect, test } from "bun:test";

import {
  createResendNewsletter,
  NewsletterProviderError,
} from "~/src/newsletter/server/resend.server";
import type { NewsletterFetch } from "~/src/newsletter/server/resend.server";
import { handleSubscription } from "~/src/newsletter/server/subscribe";

/** Build a browser-style same-origin form request without using the network. */
function form(body: unknown, headers: Record<string, string> = {}): Request {
  return new Request("https://swarmie.example/api/subscribe", {
    method: "POST",
    headers: { "Content-Type": "application/json", Origin: "https://swarmie.example", ...headers },
    body: JSON.stringify(body),
  });
}

/** Script provider results while retaining the HTTP boundary for assertions. */
function provider(responses: (Response | Error)[]) {
  const calls: { url: string; init: RequestInit }[] = [];
  const fetcher: NewsletterFetch = async (url, init) => {
    calls.push({ url, init });
    const next = responses.shift();
    if (!next) {
      throw new Error("Unexpected provider request.");
    }
    if (next instanceof Error) {
      throw next;
    }
    return next;
  };
  return { calls, fetcher };
}

describe("newsletter form boundary", () => {
  test("normalizes email and returns a private, uncached acknowledgment", async () => {
    const emails: string[] = [];
    const response = await handleSubscription(
      form({ email: "  Alice+News@Example.COM  ", website: "" }),
      {
        async subscribe(email) {
          emails.push(email);
        },
      },
    );

    expect(emails).toEqual(["alice+news@example.com"]);
    expect(response.status).toBe(200);
    expect(await response.json()).toEqual({ ok: true });
    expect(response.headers.get("cache-control")).toBe("no-store");
  });

  test.each([
    null,
    [],
    { email: 42 },
    { email: "alice@example.com", website: {} },
    { email: "alice@example.com\nBcc:other@example.com" },
    { email: ".alice@example.com" },
    { email: "alice..news@example.com" },
    { email: "alice@-example.com" },
    { email: `${"a".repeat(65)}@example.com` },
    { email: `alice@${"example.".repeat(35)}com` },
  ])("rejects malformed input without calling the provider: %j", async (body) => {
    const response = await handleSubscription(form(body), {
      async subscribe() {
        throw new Error("Must not be called.");
      },
    });
    expect(response.status).toBe(400);
  });

  test("rejects malformed JSON before resolving provider availability", async () => {
    const request = new Request("https://swarmie.example/api/subscribe", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: "{",
    });
    const response = await handleSubscription(request);
    expect(response.status).toBe(400);
  });

  test("enforces the actual byte limit even when declared Content-Length is small", async () => {
    const response = await handleSubscription(
      form({ email: "a".repeat(5000) }, { "Content-Length": "10" }),
    );
    expect(response.status).toBe(413);
  });

  test("rejects cross-origin and non-JSON browser submissions", async () => {
    const crossOrigin = form({ email: "alice@example.com" }, { Origin: "https://other.example" });
    const crossSite = form({ email: "alice@example.com" }, { "Sec-Fetch-Site": "cross-site" });
    const nonJson = form({ email: "alice@example.com" }, { "Content-Type": "text/plain" });

    const responses = await Promise.all(
      [crossOrigin, crossSite, nonJson].map(async (request) => await handleSubscription(request)),
    );
    expect(responses.map((response) => response.status)).toEqual([403, 403, 415]);
  });

  test("does not allow GET to mutate subscriptions", async () => {
    const response = await handleSubscription(new Request("https://swarmie.example/api/subscribe"));
    expect(response.status).toBe(405);
  });

  test("a populated honeypot returns acknowledgment without a configured provider", async () => {
    const response = await handleSubscription(
      form({ email: "alice@example.com", website: "https://spam.example" }),
    );
    expect(response.status).toBe(200);
    expect(await response.json()).toEqual({ ok: true });
  });

  test("missing configuration is an honest service-unavailable response", async () => {
    const response = await handleSubscription(form({ email: "alice@example.com" }));
    expect(response.status).toBe(503);
    expect(await response.json()).toEqual({
      ok: false,
      error: "We could not save your email right now. Please try again later.",
    });
  });

  test("provider exceptions do not leak credentials, addresses, or provider details", async () => {
    const response = await handleSubscription(form({ email: "alice@example.com" }), {
      async subscribe() {
        throw new Error("re_secret: provider refused alice@example.com");
      },
    });
    expect(response.status).toBe(503);
    const text = await response.text();
    expect(text).not.toContain("re_secret");
    expect(text).not.toContain("alice@example.com");
    expect(text).not.toContain("provider refused");
  });
});

describe("Resend newsletter adapter", () => {
  test("creates a missing contact with its segment and never writes subscription flags", async () => {
    const fake = provider([
      new Response(null, { status: 404 }),
      Response.json({ id: "contact-1" }, { status: 201 }),
    ]);
    await createResendNewsletter({
      apiKey: "test-key",
      segmentId: "segment-1",
      fetcher: fake.fetcher,
    }).subscribe("alice+news@example.com");

    expect(fake.calls.map((call) => call.url)).toEqual([
      "https://api.resend.com/contacts/alice%2Bnews%40example.com",
      "https://api.resend.com/contacts",
    ]);
    const [, creation] = fake.calls;
    expect(creation?.init.method).toBe("POST");
    expect(creation?.init.body).toBe(
      JSON.stringify({ email: "alice+news@example.com", segments: [{ id: "segment-1" }] }),
    );
    expect(creation?.init.signal).toBeInstanceOf(AbortSignal);
    expect(creation?.init.signal).toBe(fake.calls[0]?.init.signal);
    expect(creation?.init.redirect).toBe("error");
    expect(creation?.init.headers).toEqual({
      Authorization: "Bearer test-key",
      "Content-Type": "application/json",
    });
  });

  test.each([true, false])(
    "an existing contact without a segment needs no mutation (unsubscribed=%s)",
    async (unsubscribed) => {
      const fake = provider([Response.json({ id: "contact-1", unsubscribed })]);
      await createResendNewsletter({ apiKey: "test-key", fetcher: fake.fetcher }).subscribe(
        "alice@example.com",
      );
      expect(fake.calls).toHaveLength(1);
    },
  );

  test("an existing opt-out skips segment assignment as well as resubscription", async () => {
    const fake = provider([Response.json({ id: "contact-1", unsubscribed: true })]);
    await createResendNewsletter({
      apiKey: "test-key",
      segmentId: "segment-1",
      fetcher: fake.fetcher,
    }).subscribe("alice@example.com");
    expect(fake.calls).toHaveLength(1);
  });

  test("an existing subscriber can join a segment without updating global preferences", async () => {
    const fake = provider([
      Response.json({ id: "contact-1", unsubscribed: false }),
      Response.json({ id: "segment-1" }),
    ]);
    await createResendNewsletter({
      apiKey: "test-key",
      segmentId: "segment-1",
      fetcher: fake.fetcher,
    }).subscribe("alice@example.com");
    expect(fake.calls[1]?.url).toBe("https://api.resend.com/contacts/contact-1/segments/segment-1");
    expect(fake.calls[1]?.init.method).toBe("POST");
    expect(fake.calls[1]?.init.body).toBeUndefined();
  });

  test("a duplicate create rechecks the contact and preserves a concurrent opt-out", async () => {
    const fake = provider([
      new Response(null, { status: 404 }),
      new Response(null, { status: 409 }),
      Response.json({ id: "contact-1", unsubscribed: true }),
    ]);
    await createResendNewsletter({
      apiKey: "test-key",
      segmentId: "segment-1",
      fetcher: fake.fetcher,
    }).subscribe("alice@example.com");
    expect(fake.calls).toHaveLength(3);
    expect(fake.calls.every((call) => call.init.method !== "PATCH")).toBe(true);
  });

  test("a conflict does not count as success unless the contact can be found", async () => {
    const fake = provider([
      new Response(null, { status: 404 }),
      new Response(null, { status: 409 }),
      new Response(null, { status: 404 }),
    ]);
    expect(
      await rejected(
        createResendNewsletter({ apiKey: "test-key", fetcher: fake.fetcher }).subscribe(
          "alice@example.com",
        ),
      ),
    ).toBeInstanceOf(NewsletterProviderError);
  });

  test("duplicate segment assignment is accepted only after confirming membership", async () => {
    const fake = provider([
      Response.json({ id: "contact-1", unsubscribed: false }),
      new Response(null, { status: 409 }),
      Response.json({ data: [{ id: "segment-1" }] }),
    ]);
    await createResendNewsletter({
      apiKey: "test-key",
      segmentId: "segment-1",
      fetcher: fake.fetcher,
    }).subscribe("alice@example.com");
    expect(fake.calls[2]?.url).toBe("https://api.resend.com/contacts/contact-1/segments?limit=100");
  });

  test.each([
    new Response(null, { status: 401 }),
    Response.json({ id: "contact-1" }),
    Response.json({ id: "contact-1", unsubscribed: "false" }),
    new Error("Request timed out"),
  ])("lookup failures never fall through to creating a contact", async (result) => {
    const fake = provider([result]);
    expect(
      await rejected(
        createResendNewsletter({ apiKey: "test-key", fetcher: fake.fetcher }).subscribe(
          "alice@example.com",
        ),
      ),
    ).toBeInstanceOf(NewsletterProviderError);
    expect(fake.calls).toHaveLength(1);
  });

  test("an invalid successful creation response does not claim a saved subscription", async () => {
    const fake = provider([new Response(null, { status: 404 }), Response.json({})]);
    expect(
      await rejected(
        createResendNewsletter({ apiKey: "test-key", fetcher: fake.fetcher }).subscribe(
          "alice@example.com",
        ),
      ),
    ).toBeInstanceOf(NewsletterProviderError);
  });

  test("transport failures leave the adapter as safe typed errors without retrying", async () => {
    const privateDetail = "re_private: alice@example.com provider request";
    const fake = provider([new Error(privateDetail)]);
    try {
      await createResendNewsletter({ apiKey: "test-key", fetcher: fake.fetcher }).subscribe(
        "alice@example.com",
      );
      throw new Error("Expected the provider operation to fail.");
    } catch (error) {
      expect(error).toBeInstanceOf(NewsletterProviderError);
      expect(error).toMatchObject({ stage: "lookup", reason: "network", status: null });
      expect(JSON.stringify(error)).not.toContain(privateDetail);
      expect(String(error)).not.toContain("alice@example.com");
    }
    expect(fake.calls).toHaveLength(1);
  });

  test("rate limits preserve safe status without retrying or reading the response body", async () => {
    const response = Response.json({ detail: "private-provider-reason" }, { status: 429 });
    const fake = provider([response]);
    expect(
      await rejected(
        createResendNewsletter({ apiKey: "test-key", fetcher: fake.fetcher }).subscribe(
          "alice@example.com",
        ),
      ),
    ).toMatchObject({ stage: "lookup", reason: "http", status: 429 });
    expect(response.bodyUsed).toBe(false);
    expect(fake.calls).toHaveLength(1);
  });

  test.each([
    new Response("private-non-json-body"),
    Response.json({ id: "", unsubscribed: false, private: "private-profile-detail" }),
    Response.json({ id: "private-profile-detail", unsubscribed: "false" }),
  ])("invalid provider receipts never retain their input in typed errors", async (response) => {
    const fake = provider([response]);
    try {
      await createResendNewsletter({ apiKey: "test-key", fetcher: fake.fetcher }).subscribe(
        "alice@example.com",
      );
      throw new Error("Expected the provider receipt to fail validation.");
    } catch (error) {
      expect(error).toBeInstanceOf(NewsletterProviderError);
      expect(error).toMatchObject({ stage: "lookup", reason: "invalid-response", status: null });
      expect(JSON.stringify(error)).not.toContain("private-");
      expect(String(error)).not.toContain("private-");
    }
    expect(fake.calls).toHaveLength(1);
  });

  test("failed segment writes only confirm membership and never retry the mutation", async () => {
    const fake = provider([
      Response.json({ id: "contact-1", unsubscribed: false }),
      new Response(null, { status: 429 }),
      Response.json({ data: [{ id: "another-segment" }] }),
    ]);
    expect(
      await rejected(
        createResendNewsletter({
          apiKey: "test-key",
          segmentId: "segment-1",
          fetcher: fake.fetcher,
        }).subscribe("alice@example.com"),
      ),
    ).toMatchObject({ stage: "segment", reason: "http", status: 429 });
    expect(fake.calls).toHaveLength(3);
    expect(fake.calls.filter((call) => call.init.method === "POST")).toHaveLength(1);
    expect(fake.calls.every((call) => call.init.signal === fake.calls[0]?.init.signal)).toBe(true);
  });
});

/** Await rejection directly; Bun types asynchronous matchers as void. */
async function rejected(operation: Promise<unknown>): Promise<unknown> {
  const [result] = await Promise.allSettled([operation]);
  if (result?.status !== "rejected") {
    throw new Error("Expected the operation to reject");
  }
  return result.reason;
}
