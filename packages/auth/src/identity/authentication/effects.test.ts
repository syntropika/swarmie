import { expect, test } from "bun:test";

import { SwarmieClient } from "@swarmie/api";
import { QueryClient } from "@tanstack/react-query";

import { authQueryKeys } from "~/src/identity/authentication/cache";
import { createAuthEffects, runAuthEffect } from "~/src/identity/authentication/effects";
import { authError } from "~/src/identity/authentication/errors";
import { clearAccountQueries } from "~/src/platform/account-cache";
import { fixtureUser, rejectionOf, requestUrl, requestBody } from "~/src/test-support";

const hostQueryKeys = {
  list: (userId: string) => ["account", "host-resource", userId] as const,
  members: (userId: string, resourceId: string) =>
    ["account", "host-resource", userId, resourceId] as const,
};

const requestOptions: PublicKeyCredentialRequestOptionsJSON = {
  challenge: "AQID",
  rpId: "app.swarmie.ai",
  userVerification: "required",
};
const creationOptions: PublicKeyCredentialCreationOptionsJSON = {
  challenge: "AQID",
  rp: { id: "app.swarmie.ai", name: "Swarmie" },
  user: { id: "BAUG", name: "member@example.test", displayName: "Member" },
  pubKeyCredParams: [{ type: "public-key", alg: -7 }],
};
const assertion: AuthenticationResponseJSON = {
  type: "public-key",
  id: "BAUG",
  rawId: "BAUG",
  clientExtensionResults: {},
  response: {
    clientDataJSON: "AQID",
    authenticatorData: "BAUG",
    signature: "BwgJ",
    userHandle: "CgsM",
  },
};
const registration: RegistrationResponseJSON = {
  type: "public-key",
  id: "BAUG",
  rawId: "BAUG",
  clientExtensionResults: {},
  response: {
    clientDataJSON: "AQID",
    attestationObject: "BAUG",
    authenticatorData: "BwgJ",
    publicKeyAlgorithm: -7,
    transports: ["internal"],
  },
};

/** Return a deterministic transport response without touching a provider or a browser credential. */
function responding(payload: unknown, status = 200) {
  let requests = 0;
  const client = new SwarmieClient({
    fetch: async () => {
      requests += 1;
      return Response.json(payload, { status });
    },
  });
  return { effects: createAuthEffects(client), requests: () => requests };
}

test("signup trims the email only and decodes its verification outcome without provider data", async () => {
  const client = new SwarmieClient({
    fetch: async (input, init) => {
      expect(requestUrl(input)).toBe("/auth/signup");
      expect(requestBody(init)).toEqual({
        email: "new@example.test",
        password: " fixture-password ",
      });
      return Response.json({
        status: "email_verification_required",
        challenge: "opaque-challenge",
        pendingToken: "fixture-private-provider-token",
      });
    },
  });
  const outcome = await runAuthEffect(
    createAuthEffects(client).signUpWithPassword(" new@example.test ", " fixture-password "),
  );
  expect(outcome).toMatchObject({
    status: "email_verification_required",
    challenge: "opaque-challenge",
  });
  expect(Object.keys(outcome).toSorted()).toEqual(["challenge", "status"]);
});

test("signup rejects malformed success and returns safe recovery feedback for rejected creation", async () => {
  const malformed = responding({ status: "accepted", privateToken: "fixture-private-token" });
  expect(
    await rejectionOf(
      runAuthEffect(malformed.effects.signUpWithPassword("new@example.test", "fixture-password")),
    ),
  ).toMatchObject({ reason: "invalid-response" });
  expect(malformed.requests()).toBe(1);

  await Promise.all(
    [400, 409, 422].map(async (status) => {
      const rejected = responding({ message: "fixture-private-account-details" }, status);
      const failure = await runAuthEffect(
        rejected.effects.signUpWithPassword("new@example.test", "fixture-password"),
      ).catch((error: unknown) => error);
      expect(authError(failure, "sign-up")).toEqual({
        tone: "error",
        title: "error.signUpTitle",
        message: "error.signUp",
      });
      expect(rejected.requests()).toBe(1);
    }),
  );
});

test("authentication schemas reject malformed outcomes without retaining their payload", async () => {
  const privateMarker = "fixture-private-provider-payload";
  await Promise.all(
    [
      null,
      {},
      { status: "unknown", privateMarker },
      { status: "email_verification_required", challenge: "", privateMarker },
      { status: "email_verification_required", challenge: 123, privateMarker },
      { status: "email_verification_required", challenge: "x".repeat(1025), privateMarker },
    ].map(async (value) => {
      const fixture = responding(value);
      const failure = await runAuthEffect(
        fixture.effects.signInWithPassword("member@example.test", "fixture-password"),
      ).catch((error: unknown) => error);
      expect(failure).toMatchObject({
        _tag: "RequestError",
        reason: "invalid-response",
        status: null,
      });
      expect(JSON.stringify(failure)).not.toContain(privateMarker);
      expect(failure).not.toHaveProperty("cause");
      expect(fixture.requests()).toBe(1);
    }),
  );
});

test("decoded successful outcomes contain only the public state-transition fields", async () => {
  await Promise.all(
    [
      { status: "authenticated" },
      { status: "email_verification_required", challenge: "opaque-challenge" },
      { status: "additional_verification_required" },
    ].map(async (result) => {
      const fixture = responding({ ...result, unexpectedToken: "fixture-private-provider-token" });
      const outcome = await runAuthEffect(fixture.effects.verifyMagicLink("opaque"));
      expect(outcome).toMatchObject(result);
      expect(Object.keys(outcome).toSorted()).toEqual(Object.keys(result).toSorted());
    }),
  );
});

test("an empty successful HTTP response is rejected by the authentication schema", async () => {
  const client = new SwarmieClient({ fetch: async () => new Response(null, { status: 204 }) });
  expect(
    await rejectionOf(runAuthEffect(createAuthEffects(client).verifyMagicLink("opaque"))),
  ).toMatchObject({
    _tag: "RequestError",
    reason: "invalid-response",
    status: null,
  });
});

test("typed transport failures preserve status but discard upstream bodies and causes", async () => {
  await Promise.all(
    [401, 403, 429, 503].map(async (status) => {
      const fixture = responding({ error: { message: "fixture-private-provider-body" } }, status);
      const failure = await runAuthEffect(fixture.effects.verifyMagicLink("opaque")).catch(
        (error: unknown) => error,
      );
      expect(failure).toMatchObject({ _tag: "RequestError", reason: "http", status });
      expect(JSON.stringify(failure)).not.toContain("fixture-private-provider-body");
      expect(failure).not.toHaveProperty("response");
      expect(failure).not.toHaveProperty("cause");
      expect(fixture.requests()).toBe(1);
    }),
  );
});

test("network failures are typed and do not expose arbitrary exception messages", async () => {
  let attempts = 0;
  const client = new SwarmieClient({
    fetch: async () => {
      attempts += 1;
      throw new Error("fixture-sensitive-network-detail");
    },
  });
  const failure = await runAuthEffect(
    createAuthEffects(client).requestMagicLink("member@example.test"),
  ).catch((error: unknown) => error);
  expect(failure).toMatchObject({ _tag: "RequestError", reason: "network", status: null });
  expect(JSON.stringify(failure)).not.toContain("fixture-sensitive-network-detail");
  expect(attempts).toBe(1);
});

/** Capture the exact order of server preparation, browser ceremony, and server verification. */
function passkeyFixture(
  controller?: AbortController,
  abortAt?: "options" | "device",
  verifyStatus = 200,
) {
  const events: string[] = [];
  const client = new SwarmieClient({
    fetch: async (input, init) => {
      const path = requestUrl(input);
      events.push(path);
      if (path === "/api/me") {
        return Response.json({ user: fixtureUser, csrf_token: "fixture-csrf" });
      }
      if (path.endsWith("/options")) {
        if (abortAt === "options") {
          controller?.abort();
        }
        return Response.json({
          challenge: "opaque-challenge",
          public_key: path.startsWith("/api/") ? creationOptions : requestOptions,
        });
      }
      const body = requestBody(init);
      expect(body).toMatchObject({
        challenge: "opaque-challenge",
        credential: path.startsWith("/api/") ? registration : assertion,
      });
      if (path.startsWith("/api/")) {
        return new Response(null, { status: verifyStatus === 200 ? 204 : verifyStatus });
      }
      return Response.json({ status: "authenticated" }, { status: verifyStatus });
    },
  });
  const effects = createAuthEffects(client, {
    authenticate: async (options, signal) => {
      events.push("device.authenticate");
      expect(options).toEqual(requestOptions);
      expect(signal).toBeInstanceOf(AbortSignal);
      expect(signal?.aborted).toBe(controller?.signal.aborted ?? false);
      if (abortAt === "device") {
        controller?.abort();
      }
      return assertion;
    },
    create: async (options, signal) => {
      events.push("device.create");
      expect(options).toEqual(creationOptions);
      expect(signal).toBeInstanceOf(AbortSignal);
      expect(signal?.aborted).toBe(controller?.signal.aborted ?? false);
      if (abortAt === "device") {
        controller?.abort();
      }
      return registration;
    },
  });
  return { effects, events };
}

test("passkey authentication and enrollment preserve ordered one-shot proof exchange", async () => {
  const login = passkeyFixture();
  expect(await runAuthEffect(login.effects.signInWithPasskey("member@example.test"))).toEqual({
    status: "authenticated",
  });
  expect(login.events).toEqual([
    "/auth/passkey/options",
    "device.authenticate",
    "/auth/passkey/verify",
  ]);
  const enrollment = passkeyFixture();
  await runAuthEffect(enrollment.effects.registerPasskey("Personal laptop"));
  expect(enrollment.events).toEqual([
    "/api/me",
    "/api/passkeys/registration/options",
    "device.create",
    "/api/passkeys/registration/verify",
  ]);
});

test("aborting before preparation, after options, or after the ceremony never submits verification", async () => {
  await Promise.all(
    (["before", "options", "device"] as const).map(async (stage) => {
      await Promise.all(
        (["login", "enrollment"] as const).map(async (kind) => {
          const controller = new AbortController();
          if (stage === "before") {
            controller.abort();
          }
          const fixture = passkeyFixture(controller, stage === "before" ? undefined : stage);
          const effect =
            kind === "login"
              ? fixture.effects.signInWithPasskey("member@example.test", controller.signal)
              : fixture.effects.registerPasskey("Personal laptop", controller.signal);
          expect(await rejectionOf(runAuthEffect(effect))).toMatchObject({
            _tag: "AuthPasskeyError",
            code: "cancelled",
          });
          expect(fixture.events.some((event) => event.endsWith("/verify"))).toBe(false);
          if (stage === "before") {
            expect(fixture.events).toHaveLength(0);
          }
          if (stage === "options") {
            expect(fixture.events.some((event) => event.startsWith("device."))).toBe(false);
          }
        }),
      );
    }),
  );
});

test("ambiguous passkey verification failures never retry the ceremony or its consumed proof", async () => {
  const fixture = passkeyFixture(undefined, undefined, 503);
  expect(
    await rejectionOf(runAuthEffect(fixture.effects.signInWithPasskey("member@example.test"))),
  ).toMatchObject({ _tag: "RequestError", status: 503 });
  expect(fixture.events).toEqual([
    "/auth/passkey/options",
    "device.authenticate",
    "/auth/passkey/verify",
  ]);
});

test("an identity transition clears private cached data and prevents late old-account query results", async () => {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  client.setQueryData(authQueryKeys.session, { id: "old-account" });
  client.setQueryData(hostQueryKeys.list("old-account"), [{ id: "private-workspace" }]);
  client.setQueryData(["public", "metadata"], { version: "fixture" });
  const passkeyRead = Promise.withResolvers<string[]>();
  const workspaceRead = Promise.withResolvers<string[]>();
  const pending = client
    .query({
      queryKey: authQueryKeys.passkeys("old-account"),
      queryFn: async () => await passkeyRead.promise,
    })
    .catch(() => {});
  const pendingWorkspace = client
    .query({
      queryKey: hostQueryKeys.members("old-account", "private-workspace"),
      queryFn: async () => await workspaceRead.promise,
    })
    .catch(() => {});
  await clearAccountQueries(client);
  client.setQueryData(authQueryKeys.session, { id: "new-account" });
  passkeyRead.resolve(["old-account-key"]);
  workspaceRead.resolve(["old-account-membership"]);
  await Promise.all([pending, pendingWorkspace]);
  expect(client.getQueryData<{ id: string }>(authQueryKeys.session)).toEqual({ id: "new-account" });
  expect(client.getQueryData<object>(authQueryKeys.passkeys("old-account"))).toBeUndefined();
  expect(client.getQueryData<object>(hostQueryKeys.list("old-account"))).toBeUndefined();
  expect(
    client.getQueryData<object>(hostQueryKeys.members("old-account", "private-workspace")),
  ).toBeUndefined();
  expect(client.getQueryData<{ version: string }>(["public", "metadata"])).toEqual({
    version: "fixture",
  });
  client.clear();
});
