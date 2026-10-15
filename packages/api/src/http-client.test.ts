import { describe, expect, test } from "bun:test";

import * as Cause from "effect/Cause";
import * as Effect from "effect/Effect";
import * as Either from "effect/Either";
import * as Exit from "effect/Exit";

import { SwarmieClient } from "./http-client";
import { RequestError } from "./request-error";
import { requestUrl, requestBody, userFixture, workspaceFixture, rejected } from "./test-helpers";

/** Inspect expected failures directly, rather than a Promise runtime’s FiberFailure wrapper. */
async function run<E extends Error>(operation: Effect.Effect<unknown, E>): Promise<unknown> {
  const result = await Effect.runPromise(Effect.either(operation));
  if (Either.isLeft(result)) {
    throw result.left;
  }
  return result.right;
}

describe("session-aware requests", () => {
  test("loads CSRF protection before a mutation and includes the session cookie", async () => {
    const requests: { url: string; init: RequestInit | undefined }[] = [];
    const client = new SwarmieClient({
      baseUrl: "https://swarmie.test",
      fetch: async (input, init) => {
        requests.push({ url: requestUrl(input), init });
        return requests.length === 1
          ? Response.json({ user: userFixture, csrf_token: "session-csrf" })
          : Response.json(workspaceFixture);
      },
    });

    await run(client.createWorkspace("Team"));

    expect(requests.map(({ url }) => url)).toEqual([
      "https://swarmie.test/api/me",
      "https://swarmie.test/api/workspaces",
    ]);
    expect(requests.every(({ init }) => init?.credentials === "include")).toBe(true);
    expect(new Headers(requests[1]?.init?.headers).get("X-CSRF-Token")).toBe("session-csrf");
  });

  test("does not submit a mutation when session authentication fails", async () => {
    let requests = 0;
    const client = new SwarmieClient({
      fetch: async () => {
        requests += 1;
        return Response.json({ error: "unauthorized" }, { status: 401 });
      },
    });

    expect(await rejected(run(client.createWorkspace("Team")))).toBeInstanceOf(RequestError);
    expect(requests).toBe(1);
  });

  test("does not retry a failed mutation automatically", async () => {
    let requests = 0;
    const client = new SwarmieClient({
      fetch: async () => {
        requests += 1;
        return requests === 1
          ? Response.json({ user: userFixture, csrf_token: "session-csrf" })
          : Response.json({ error: "unavailable" }, { status: 503 });
      },
    });

    expect(await rejected(run(client.createWorkspace("Team")))).toMatchObject({ status: 503 });
    expect(requests).toBe(2);
  });

  test("recovers from a session rotated in another tab without replaying a rejected mutation", async () => {
    let reads = 0;
    const submittedTokens: (string | null)[] = [];
    const client = new SwarmieClient({
      fetch: async (input, init) => {
        if (requestUrl(input) === "/api/me") {
          return Response.json({ user: userFixture, csrf_token: `csrf-${(reads += 1)}` });
        }
        submittedTokens.push(new Headers(init?.headers).get("X-CSRF-Token"));
        return submittedTokens.length === 1
          ? Response.json({ error: "forbidden" }, { status: 403 })
          : new Response(null, { status: 204 });
      },
    });

    await run(client.me());
    expect(await rejected(run(client.leaveWorkspace("workspace")))).toMatchObject({ status: 403 });
    expect(submittedTokens).toEqual(["csrf-1"]);

    await run(client.leaveWorkspace("workspace"));
    expect(reads).toBe(2);
    expect(submittedTokens).toEqual(["csrf-1", "csrf-2"]);
  });

  test("clears the CSRF token after logout", async () => {
    const urls: string[] = [];
    const client = new SwarmieClient({
      fetch: async (url) => {
        urls.push(requestUrl(url));
        return requestUrl(url).endsWith("/api/me")
          ? Response.json({ user: userFixture, csrf_token: "session-csrf" })
          : new Response(null, { status: 204 });
      },
    });

    await run(client.logout());
    await run(client.leaveWorkspace("workspace"));

    expect(urls).toEqual(["/api/me", "/auth/logout", "/api/me", "/api/workspaces/workspace/leave"]);
  });
});

describe("custom authentication", () => {
  test("signup stays public and refreshes CSRF only after email verification creates a session", async () => {
    const requests: { url: string; init: RequestInit | undefined }[] = [];
    let sessionReads = 0;
    const client = new SwarmieClient({
      fetch: async (input, init) => {
        const url = requestUrl(input);
        requests.push({ url, init });
        if (url === "/api/me") {
          return Response.json({ user: userFixture, csrf_token: `csrf-${(sessionReads += 1)}` });
        }
        if (url === "/auth/signup") {
          return Response.json(
            { status: "email_verification_required", challenge: "signup-verification" },
            { status: 202 },
          );
        }
        if (url === "/auth/email-verification") {
          return Response.json({ status: "authenticated" });
        }
        return new Response(null, { status: 204 });
      },
    });

    await run(client.me());
    const signup = client.signUpWithPassword("new@example.test", " fixture-password ");
    expect(requests).toHaveLength(1);
    expect(await run(signup)).toEqual({
      status: "email_verification_required",
      challenge: "signup-verification",
    });
    const signupRequest = requests[1]?.init;
    expect(signupRequest?.credentials).toBe("include");
    expect(new Headers(signupRequest?.headers).has("X-CSRF-Token")).toBe(false);
    expect(requestBody(signupRequest?.body)).toEqual({
      email: "new@example.test",
      password: " fixture-password ",
    });
    await run(client.leaveWorkspace("old-workspace"));
    expect(sessionReads).toBe(1);

    await run(client.verifyEmail("signup-verification", "123456"));
    await run(client.leaveWorkspace("new-workspace"));
    expect(sessionReads).toBe(2);
    expect(requests.map(({ url }) => url)).toEqual([
      "/api/me",
      "/auth/signup",
      "/api/workspaces/old-workspace/leave",
      "/auth/email-verification",
      "/api/me",
      "/api/workspaces/new-workspace/leave",
    ]);
    expect(new Headers(requests.at(-1)?.init?.headers).get("X-CSRF-Token")).toBe("csrf-2");
  });

  test("signup never retries ambiguous creation or exposes rejected provider bodies", async () => {
    await Promise.all(
      [400, 409, 429, 503].map(async (status) => {
        let calls = 0;
        const response = new Response("fixture-private-account-details", { status });
        const client = new SwarmieClient({
          fetch: async () => {
            calls += 1;
            return response;
          },
        });
        const error = await rejected(
          run(client.signUpWithPassword("new@example.test", "fixture-password")),
        );
        expect(error).toMatchObject({ reason: "http", status });
        expect(response.bodyUsed).toBe(false);
        expect(JSON.stringify(error)).not.toContain("fixture-private-account-details");
        expect(calls).toBe(1);
      }),
    );
  });

  test("can establish a cookie session without first requesting a protected endpoint", async () => {
    const requests: { url: string; init: RequestInit | undefined }[] = [];
    const client = new SwarmieClient({
      baseUrl: "https://api-dev.swarmie.ai",
      fetch: async (input, init) => {
        requests.push({ url: requestUrl(input), init });
        return Response.json({ status: "authenticated" });
      },
    });
    expect(await run(client.signInWithPassword("person@example.com", "password"))).toEqual({
      status: "authenticated",
    });
    expect(requests).toHaveLength(1);
    expect(requests[0]?.url).toBe("https://api-dev.swarmie.ai/auth/password");
    expect(requests[0]?.init?.credentials).toBe("include");
    expect(new Headers(requests[0]?.init?.headers).get("Content-Type")).toBe("application/json");
    expect(new Headers(requests[0]?.init?.headers).has("X-CSRF-Token")).toBe(false);
  });

  test("refreshes CSRF protection after replacing a session through direct authentication", async () => {
    let reads = 0;
    const mutations: Headers[] = [];
    const client = new SwarmieClient({
      fetch: async (input, init) => {
        if (requestUrl(input) === "/api/me") {
          return Response.json({ user: userFixture, csrf_token: `csrf-${(reads += 1)}` });
        }
        if (requestUrl(input) === "/auth/password") {
          return Response.json({ status: "authenticated" });
        }
        mutations.push(new Headers(init?.headers));
        return new Response(null, { status: 204 });
      },
    });
    await run(client.me());
    await run(client.signInWithPassword("person@example.com", "password"));
    await run(client.leaveWorkspace("workspace"));
    expect(reads).toBe(2);
    expect(mutations[0]?.get("X-CSRF-Token")).toBe("csrf-2");
  });

  test("does not retry consumed email challenges or account recovery requests", async () => {
    const calls: string[] = [];
    const client = new SwarmieClient({
      fetch: async (input) => {
        calls.push(requestUrl(input));
        return Response.json({ error: "unavailable" }, { status: 503 });
      },
    });
    expect(await rejected(run(client.verifyMagicLink("opaque")))).toMatchObject({ status: 503 });
    expect(await rejected(run(client.requestPasswordReset("person@example.com")))).toMatchObject({
      status: 503,
    });
    expect(calls).toEqual(["/auth/magic-link/verify", "/auth/password/reset"]);
  });
});

describe("native Effect transport", () => {
  test("constructing authentication, logout, and reset effects does not fetch or alter CSRF state", async () => {
    const requests: string[] = [];
    const tokens: (string | null)[] = [];
    const client = new SwarmieClient({
      fetch: async (input, init) => {
        const path = requestUrl(input);
        requests.push(path);
        if (path === "/api/me") {
          return Response.json({ user: userFixture, csrf_token: "existing-session" });
        }
        tokens.push(new Headers(init?.headers).get("X-CSRF-Token"));
        return new Response(null, { status: 204 });
      },
    });
    await run(client.me());
    const operations = [
      client.logout(),
      client.signInWithPassword("member@example.test", "fixture-password"),
      client.verifyMagicLink("fixture-proof"),
      client.resetPassword("fixture-proof", "fixture-password"),
      client.createWorkspace("Later"),
    ];
    expect(operations.every(Effect.isEffect)).toBe(true);
    expect(requests).toEqual(["/api/me"]);
    await run(client.leaveWorkspace("workspace"));
    expect(requests).toEqual(["/api/me", "/api/workspaces/workspace/leave"]);
    expect(tokens).toEqual(["existing-session"]);
  });

  test("interruption aborts in-flight CSRF acquisition without submitting or replaying the write", async () => {
    const requests: string[] = [];
    const { promise: started, resolve: notifyStarted } = Promise.withResolvers<AbortSignal>();
    const client = new SwarmieClient({
      fetch: async (input, init) => {
        const { promise, reject } = Promise.withResolvers<Response>();
        requests.push(requestUrl(input));
        const signal = init?.signal;
        if (!signal) {
          throw new Error("Expected an Effect interruption signal");
        }
        notifyStarted(signal);
        signal.addEventListener(
          "abort",
          () => {
            reject(new Error("fixture-private-abort-detail"));
          },
          { once: true },
        );
        return await promise;
      },
    });
    const controller = new AbortController();
    const result = Effect.runPromiseExit(client.createWorkspace("Team"), {
      signal: controller.signal,
    });
    const fetchSignal = await started;
    controller.abort();
    const exit = await result;
    expect(Exit.isFailure(exit)).toBe(true);
    if (Exit.isFailure(exit)) {
      expect(Cause.isInterruptedOnly(exit.cause)).toBe(true);
    }
    expect(fetchSignal.aborted).toBe(true);
    expect(requests).toEqual(["/api/me"]);
  });

  test("network and malformed JSON failures expose only safe classifications and never retry", async () => {
    const marker = "fixture-private-network-body-password";
    await Promise.all(
      (["network", "invalid-response"] as const).map(async (reason) => {
        let requests = 0;
        const client = new SwarmieClient({
          fetch: async () => {
            requests += 1;
            if (reason === "network") {
              throw new Error(marker);
            }
            return new Response(marker, { headers: { "Content-Type": "application/json" } });
          },
        });
        const result = await Effect.runPromise(Effect.either(client.verifyMagicLink(marker)));
        expect(Either.isLeft(result)).toBe(true);
        if (Either.isLeft(result)) {
          expect(result.left).toBeInstanceOf(RequestError);
          expect(result.left).toMatchObject({ _tag: "RequestError", reason, status: null });
          expect(JSON.stringify(result.left)).not.toContain(marker);
          expect(result.left.cause).toBeUndefined();
          expect("response" in result.left).toBe(false);
        }
        expect(requests).toBe(1);
      }),
    );
  });

  test("HTTP failures preserve status without reading upstream error bodies", async () => {
    const response = new Response("fixture-private-upstream-body", { status: 429 });
    const client = new SwarmieClient({ fetch: async () => response });
    const failure = await Effect.runPromise(
      Effect.flip(client.requestMagicLink("member@example.test")),
    );
    expect(failure).toMatchObject({ _tag: "RequestError", reason: "http", status: 429 });
    expect(response.bodyUsed).toBe(false);
    expect(JSON.stringify(failure)).not.toContain("fixture-private-upstream-body");
  });

  test("a malformed CSRF response cannot authorize a protected mutation", async () => {
    const calls: string[] = [];
    const client = new SwarmieClient({
      fetch: async (input) => {
        calls.push(requestUrl(input));
        return Response.json({ user: userFixture, csrf_token: null });
      },
    });
    const failure = await Effect.runPromise(Effect.flip(client.createWorkspace("Team")));
    expect(failure).toMatchObject({ reason: "invalid-response", status: null });
    expect(calls).toEqual(["/api/me"]);
  });

  test("password reset refreshes cached CSRF only after the reset effect succeeds", async () => {
    let reads = 0;
    let submittedToken: string | null = null;
    const client = new SwarmieClient({
      fetch: async (input, init) => {
        if (requestUrl(input) === "/api/me") {
          return Response.json({ user: userFixture, csrf_token: `csrf-${(reads += 1)}` });
        }
        if (requestUrl(input).startsWith("/api/workspaces/")) {
          submittedToken = new Headers(init?.headers).get("X-CSRF-Token");
        }
        return new Response(null, { status: 204 });
      },
    });
    await run(client.me());
    await run(client.resetPassword("fixture-proof", "fixture-password"));
    await run(client.leaveWorkspace("workspace"));
    expect(reads).toBe(2);
    expect<string | null>(submittedToken).toBe("csrf-2");
  });
});

test("void commands discard successful payloads instead of leaking them into consumer state", async () => {
  const client = new SwarmieClient({
    fetch: async () =>
      Response.json({ status: "accepted", unexpectedProof: "fixture-private-proof" }),
  });
  expect(await run(client.requestMagicLink("member@example.test"))).toBeUndefined();
  expect(await run(client.requestPasswordReset("member@example.test"))).toBeUndefined();
  expect(await run(client.resetPassword("fixture-proof", "fixture-password"))).toBeUndefined();
});
