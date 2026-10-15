export type PasskeySupport = "available" | "insecure-context" | "unsupported";

export type PasskeyErrorCode =
  | "cancelled"
  | "already-registered"
  | "unavailable"
  | "configuration"
  | "failed";

/** A browser ceremony failed before the API could verify it; messages contain no credential data. */
export class PasskeyError extends Error {
  constructor(
    readonly code: PasskeyErrorCode,
    message: string,
  ) {
    super(message);
    this.name = "PasskeyError";
  }
}

/** The native browser boundary, injectable so ceremony outcomes can be verified without a device. */
export type WebAuthnPlatform = {
  support: PasskeySupport;
  parseCreationOptions: (
    options: PublicKeyCredentialCreationOptionsJSON,
  ) => PublicKeyCredentialCreationOptions;
  parseRequestOptions: (
    options: PublicKeyCredentialRequestOptionsJSON,
  ) => PublicKeyCredentialRequestOptions;
  create: (
    options: CredentialCreationOptions,
  ) => Promise<RegistrationResponseJSON | AuthenticationResponseJSON | null>;
  get: (
    options: CredentialRequestOptions,
  ) => Promise<RegistrationResponseJSON | AuthenticationResponseJSON | null>;
};

/** Detect the complete JSON-capable native API, without prompting for or enumerating passkeys. */
export function getPasskeySupport(): PasskeySupport {
  if (typeof window === "undefined") {
    return "unsupported";
  }
  if (!window.isSecureContext) {
    return "insecure-context";
  }
  if (
    typeof PublicKeyCredential === "undefined" ||
    navigator.credentials === undefined ||
    typeof navigator.credentials.create !== "function" ||
    typeof navigator.credentials.get !== "function" ||
    typeof PublicKeyCredential.parseCreationOptionsFromJSON !== "function" ||
    typeof PublicKeyCredential.parseRequestOptionsFromJSON !== "function" ||
    typeof PublicKeyCredential.prototype.toJSON !== "function"
  ) {
    return "unsupported";
  }
  return "available";
}

function nativePlatform(): WebAuthnPlatform {
  return {
    support: getPasskeySupport(),
    parseCreationOptions: (options) => PublicKeyCredential.parseCreationOptionsFromJSON(options),
    parseRequestOptions: (options) => PublicKeyCredential.parseRequestOptionsFromJSON(options),
    create: async (options) => serializeCredential(await navigator.credentials.create(options)),
    get: async (options) => serializeCredential(await navigator.credentials.get(options)),
  };
}

function serializeCredential(
  credential: Credential | null,
): RegistrationResponseJSON | AuthenticationResponseJSON | null {
  if (credential === null) {
    return null;
  }
  if (!(credential instanceof PublicKeyCredential)) {
    throw new PasskeyError("failed", "The browser returned an unexpected credential. Try again.");
  }
  return credential.toJSON();
}

function assertAvailable(support: PasskeySupport) {
  if (support === "insecure-context") {
    throw new PasskeyError(
      "unavailable",
      "Passkeys need a secure connection. Open Swarmie using HTTPS and try again.",
    );
  }
  if (support === "unsupported") {
    throw new PasskeyError(
      "unavailable",
      "This browser doesn’t support this passkey flow. Update your browser or use another sign-in method.",
    );
  }
}

function assertNotCancelled(signal?: AbortSignal) {
  if (signal?.aborted === true) {
    throw new PasskeyError("cancelled", "The passkey request was cancelled. You can try again.");
  }
}

function ceremonyError(error: unknown, operation: "create" | "get"): PasskeyError {
  if (error instanceof PasskeyError) {
    return error;
  }
  const name =
    error !== null && typeof error === "object" && "name" in error ? error.name : undefined;
  if (name === "AbortError" || name === "NotAllowedError") {
    return new PasskeyError(
      "cancelled",
      "The passkey request wasn’t completed. Try again or choose another sign-in method.",
    );
  }
  if (name === "InvalidStateError" && operation === "create") {
    return new PasskeyError(
      "already-registered",
      "This passkey is already registered. Use it to sign in or choose another authenticator.",
    );
  }
  if (name === "SecurityError") {
    return new PasskeyError(
      "configuration",
      "Passkeys aren’t available at this address. Open Swarmie from its usual app address and try again.",
    );
  }
  if (name === "NotSupportedError" || name === "ConstraintError") {
    return new PasskeyError(
      "unavailable",
      "This browser or device can’t use the requested passkey. Try another device or sign-in method.",
    );
  }
  return new PasskeyError(
    "failed",
    "We couldn’t complete the passkey request. Try again or choose another sign-in method.",
  );
}

function isRegistration(
  result: RegistrationResponseJSON | AuthenticationResponseJSON,
): result is RegistrationResponseJSON {
  return "attestationObject" in result.response;
}

function isAuthentication(
  result: RegistrationResponseJSON | AuthenticationResponseJSON,
): result is AuthenticationResponseJSON {
  return "signature" in result.response;
}

/**
 * Convert server JSON with the browser's WebAuthn implementation and return its serialized proof.
 * No transport, persistence, automatic retry, or authentication decision occurs in this adapter.
 */
export function createPasskeyClient(platform: WebAuthnPlatform) {
  return {
    async create(
      options: PublicKeyCredentialCreationOptionsJSON,
      signal?: AbortSignal,
    ): Promise<RegistrationResponseJSON> {
      assertAvailable(platform.support);
      assertNotCancelled(signal);
      try {
        const publicKey = platform.parseCreationOptions(options);
        const result = await platform.create({ publicKey, ...(signal ? { signal } : {}) });
        assertNotCancelled(signal);
        if (!result) {
          throw new PasskeyError("cancelled", "No passkey was created. You can try again.");
        }
        if (result.type !== "public-key" || !isRegistration(result)) {
          throw new PasskeyError(
            "failed",
            "The browser returned an unexpected passkey response. Try again.",
          );
        }
        return result;
      } catch (error) {
        throw ceremonyError(error, "create");
      }
    },
    async authenticate(
      options: PublicKeyCredentialRequestOptionsJSON,
      signal?: AbortSignal,
    ): Promise<AuthenticationResponseJSON> {
      assertAvailable(platform.support);
      assertNotCancelled(signal);
      try {
        const publicKey = platform.parseRequestOptions(options);
        const result = await platform.get({ publicKey, ...(signal ? { signal } : {}) });
        assertNotCancelled(signal);
        if (!result) {
          throw new PasskeyError("cancelled", "No passkey was selected. You can try again.");
        }
        if (result.type !== "public-key" || !isAuthentication(result)) {
          throw new PasskeyError(
            "failed",
            "The browser returned an unexpected passkey response. Try again.",
          );
        }
        return result;
      } catch (error) {
        throw ceremonyError(error, "get");
      }
    },
  };
}

/** Ask the device to create a credential; the API must verify the returned proof before storing it. */
export async function createPasskey(
  options: PublicKeyCredentialCreationOptionsJSON,
  signal?: AbortSignal,
): Promise<RegistrationResponseJSON> {
  return await createPasskeyClient(nativePlatform()).create(options, signal);
}

/** Ask the device for a proof; only successful API verification creates a Swarmie session. */
export async function authenticatePasskey(
  options: PublicKeyCredentialRequestOptionsJSON,
  signal?: AbortSignal,
): Promise<AuthenticationResponseJSON> {
  return await createPasskeyClient(nativePlatform()).authenticate(options, signal);
}
