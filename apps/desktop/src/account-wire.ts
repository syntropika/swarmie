import type { AccountMethods } from "@swarmie/contracts";
import Ajv from "ajv/dist/2020";

const text = { type: "string" };
const nullableText = { anyOf: [text, { type: "null" }] };
const number = { type: "number" };
const flag = { type: "boolean" };
function object(properties: Record<string, object>, optional: readonly string[] = []) {
  return {
    type: "object",
    properties,
    required: Object.keys(properties).filter((key) => !optional.includes(key)),
  };
}
const profile = object({
  id: text,
  displayName: nullableText,
  email: text,
  emailVerified: flag,
  avatarUrl: nullableText,
  revision: number,
});
const snapshot = object(
  {
    status: { enum: ["signedIn", "signedOut", "signingIn", "storageUnavailable"] },
    generation: number,
    profile: { anyOf: [profile, { type: "null" }] },
    persistence: { enum: ["secureVault", "session"] },
    loginId: nullableText,
    authorizationUrl: nullableText,
    configured: flag,
  },
  ["authorizationUrl", "configured"],
);
const session = object({
  id: text,
  installationId: text,
  authenticatedAt: text,
  createdAt: text,
  lastUsedAt: text,
  expiresAt: text,
});
const resultSchemas: Record<keyof AccountMethods, object> = {
  "account/read": snapshot,
  "account/observe": object({
    cursor: object({ ownerId: text, revision: number }),
    account: snapshot,
  }),
  "account/configure": snapshot,
  "account/login/start": object(
    {
      loginId: text,
      authorizationUrl: text,
      expiresIn: number,
      persistence: { enum: ["secureVault", "session"] },
      browserOpened: flag,
    },
    ["browserOpened"],
  ),
  "account/login/cancel": snapshot,
  "account/logout": object({ account: snapshot, remoteRevocationConfirmed: flag }),
  "account/sessions/read": object({
    current: session,
    others: { type: "array", items: session },
    nextCursor: nullableText,
  }),
  "account/sessions/revoke": snapshot,
  "account/sessions/revoke-all": snapshot,
  "account/profile/read": snapshot,
  "account/profile/update": snapshot,
  "account/profile/avatar/update": snapshot,
};
const engine = new Ajv({ strict: false });

/** Each account method selects its own response contract before returning typed data. */
export function decodeAccountResult<M extends keyof AccountMethods>(
  method: M,
  value: unknown,
): AccountMethods[M]["result"] {
  const validate = engine.compile<AccountMethods[M]["result"]>(resultSchemas[method]);
  if (!validate(value)) {
    throw new TypeError("Invalid account response.");
  }
  return value;
}
