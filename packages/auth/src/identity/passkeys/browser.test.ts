import { describe, expect, test } from "bun:test";

import { createPasskeyClient, PasskeyError } from "~/src/identity/passkeys/browser";
import type { WebAuthnPlatform } from "~/src/identity/passkeys/browser";
import { rejectionOf } from "~/src/test-support";

const creation: PublicKeyCredentialCreationOptionsJSON = {
  challenge: "AQID",
  rp: { id: "app.swarmie.ai", name: "Swarmie" },
  user: { id: "BAUG", name: "member@example.com", displayName: "Member" },
  pubKeyCredParams: [{ type: "public-key", alg: -7 }],
};

const request: PublicKeyCredentialRequestOptionsJSON = {
  challenge: "AQID",
  rpId: "app.swarmie.ai",
  allowCredentials: [{ id: "BAUG", type: "public-key" }],
  userVerification: "required",
};

const registration: RegistrationResponseJSON = {
  type: "public-key",
  id: "BAUG",
  rawId: "BAUG",
  authenticatorAttachment: "platform",
  clientExtensionResults: { credProps: { rk: true } },
  response: {
    clientDataJSON: "AQID",
    attestationObject: "BAUG",
    authenticatorData: "BwgJ",
    publicKeyAlgorithm: -7,
    transports: ["internal", "hybrid"],
  },
};

const authentication: AuthenticationResponseJSON = {
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

const parsedCreation: PublicKeyCredentialCreationOptions = {
  rp: creation.rp,
  pubKeyCredParams: creation.pubKeyCredParams,
  challenge: new Uint8Array([1, 2, 3]),
  user: { ...creation.user, id: new Uint8Array([4, 5, 6]) },
};

const parsedRequest: PublicKeyCredentialRequestOptions = {
  challenge: new Uint8Array([1, 2, 3]),
  rpId: "app.swarmie.ai",
  userVerification: "required",
  allowCredentials: [{ type: "public-key", id: new Uint8Array([4, 5, 6]) }],
};

function platform(overrides: Partial<WebAuthnPlatform> = {}): WebAuthnPlatform {
  return {
    support: "available",
    parseCreationOptions: () => parsedCreation,
    parseRequestOptions: () => parsedRequest,
    create: async () => registration,
    get: async () => authentication,
    ...overrides,
  };
}

describe("native passkey ceremony boundary", () => {
  test("passes server JSON through native conversion and preserves the complete registration proof", async () => {
    const { signal } = new AbortController();
    const calls: CredentialCreationOptions[] = [];
    const client = createPasskeyClient(
      platform({
        parseCreationOptions: (input) => {
          expect(input).toBe(creation);
          return parsedCreation;
        },
        create: async (options) => {
          calls.push(options);
          return registration;
        },
      }),
    );

    expect(await client.create(creation, signal)).toBe(registration);
    expect(calls).toEqual([{ publicKey: parsedCreation, signal }]);
  });

  test("retains the assertion, user handle and cancellation signal without changing server requirements", async () => {
    const { signal } = new AbortController();
    const calls: CredentialRequestOptions[] = [];
    const client = createPasskeyClient(
      platform({
        parseRequestOptions: (input) => {
          expect(input).toBe(request);
          return parsedRequest;
        },
        get: async (options) => {
          calls.push(options);
          return authentication;
        },
      }),
    );

    expect(await client.authenticate(request, signal)).toBe(authentication);
    expect(calls).toEqual([{ publicKey: parsedRequest, signal }]);
  });

  test("does not prompt in an insecure context or unsupported browser", async () => {
    let prompts = 0;
    await Promise.all(
      (["insecure-context", "unsupported"] as const).map(async (support) => {
        const client = createPasskeyClient(
          platform({
            support,
            get: async () => {
              prompts += 1;
              return authentication;
            },
          }),
        );
        expect(await rejectionOf(client.authenticate(request))).toMatchObject({
          code: "unavailable",
        });
      }),
    );
    expect(prompts).toBe(0);
  });

  test("does not open a cancelled prompt and discards a proof resolved after cancellation", async () => {
    const early = new AbortController();
    early.abort();
    let prompts = 0;
    const earlyClient = createPasskeyClient(
      platform({
        get: async () => {
          prompts += 1;
          return authentication;
        },
      }),
    );
    expect(await rejectionOf(earlyClient.authenticate(request, early.signal))).toMatchObject({
      code: "cancelled",
    });
    expect(prompts).toBe(0);

    const late = new AbortController();
    const lateClient = createPasskeyClient(
      platform({
        get: async () => {
          late.abort();
          return authentication;
        },
      }),
    );
    expect(await rejectionOf(lateClient.authenticate(request, late.signal))).toMatchObject({
      code: "cancelled",
    });
  });

  test("null and the wrong ceremony response cannot be passed to API verification", async () => {
    expect(
      await rejectionOf(
        createPasskeyClient(platform({ get: async () => null })).authenticate(request),
      ),
    ).toMatchObject({ code: "cancelled" });
    expect(
      await rejectionOf(
        createPasskeyClient(platform({ create: async () => authentication })).create(creation),
      ),
    ).toMatchObject({ code: "failed" });
    expect(
      await rejectionOf(
        createPasskeyClient(platform({ get: async () => registration })).authenticate(request),
      ),
    ).toMatchObject({ code: "failed" });
  });

  test("normalizes cancellation and duplicate enrollment without exposing browser internals", async () => {
    await Promise.all(
      ["AbortError", "NotAllowedError"].map(async (name) => {
        const client = createPasskeyClient(
          platform({
            get: async () => {
              throw new DOMException("internal credential details", name);
            },
          }),
        );
        expect(await rejectionOf(client.authenticate(request))).toMatchObject({
          code: "cancelled",
        });
      }),
    );
    const client = createPasskeyClient(
      platform({
        create: async () => {
          throw new DOMException("internal credential details", "InvalidStateError");
        },
      }),
    );
    try {
      await client.create(creation);
      throw new Error("Expected enrollment to fail");
    } catch (error) {
      expect(error).toBeInstanceOf(PasskeyError);
      expect(error).toMatchObject({ code: "already-registered" });
      if (!(error instanceof Error)) {
        throw new TypeError("Expected a passkey error", { cause: error });
      }
      expect(error.message).not.toContain("internal credential details");
    }
  });
});
