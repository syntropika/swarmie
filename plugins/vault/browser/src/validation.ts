/** Validate the safe private projection before it enters browser presentation State. */
import type { EnrollmentRequest, JoinProposal, VaultState } from "./index";

type Check = (value: unknown) => boolean;
function record(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
const text: Check = (value) => typeof value === "string" && value.length <= 262_144;
const bool: Check = (value) => typeof value === "boolean";
const number: Check = (value) =>
  typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
const list =
  (check: Check): Check =>
  (value) =>
    Array.isArray(value) && value.length <= 32_768 && value.every(check);
const nullable =
  (check: Check): Check =>
  (value) =>
    value === null || check(value);
const choice =
  (...values: string[]): Check =>
  (value) =>
    typeof value === "string" && values.includes(value);
const shape =
  (fields: Record<string, Check>): Check =>
  (value) =>
    record(value) && Object.entries(fields).every(([key, check]) => check(value[key]));
const bytes: Check = list((value) => number(value) && typeof value === "number" && value <= 255);
const identity = shape({ account: text, vault: text, epoch: text });
const reference = shape({ account: text, vault: text, secret: text });
const scope: Check = (value) =>
  shape({ kind: choice("unrestricted") })(value) ||
  shape({ kind: choice("only"), ids: list(text) })(value);
const metadata = shape({
  name: text,
  purpose: text,
  projects: scope,
  agents: scope,
  approval: choice("none", "operation", "conversation"),
  policy_generation: text,
  target_account: nullable(text),
});
const enrollment = shape({
  statement: shape({
    version: number,
    identity,
    recipient: text,
    signing_key: bytes,
    encryption_key: nullable(bytes),
    roles: list(text),
    approver: bytes,
    nonce: text,
    envelope_digest: nullable(bytes),
    commitment: bytes,
  }),
  signature: bytes,
});
const proposal = shape({ certificate: enrollment, chain: list(enrollment), root: bytes });
const enrollmentRequest = shape({ identity, device: text, signing: bytes, encryption: bytes });
const mutation = shape({
  latest: nullable(text),
  reference,
  expected: nullable(text),
  metadata,
  kind: choice("value", "deleted"),
  prepared_entry: text,
});
const page = shape({
  offset: number,
  total: number,
  next: nullable(number),
  previous: nullable(number),
});
const pages: Check = (value) => record(value) && Object.values(value).every(page);
const card: Check = (value) => {
  if (!shape({ id: text, conversation: nullable(text), title: text, purpose: text })(value)) {
    return false;
  }
  return (
    shape({ kind: choice("entry"), metadata, replacing: bool })(value) ||
    shape({
      kind: choice("use"),
      destination: text,
      conversationAllowed: bool,
      metadata,
      reference: text,
      provider: text,
      project: nullable(text),
      agent: nullable(text),
      mainAgent: bool,
    })(value) ||
    shape({ kind: choice("capture"), destination: text, profile: text, metadata, replacing: bool })(
      value,
    ) ||
    shape({
      kind: choice("model"),
      destination: text,
      inspection: choice("detected", "incomplete", "unavailable"),
      rules: list(text),
    })(value)
  );
};
const state = shape({
  phase: choice("unavailable", "signed-out", "locked", "unlocking", "ready", "joining"),
  pages,
  entries: list(shape({ reference: text, revision: text, metadata })),
  cards: list(card),
  cardCounts: (value) =>
    record(value) && Object.keys(value).length <= 64 && Object.values(value).every(number),
  synchronization: choice("offline", "synchronizing", "confirmed"),
  archived: bool,
  exposed: bool,
  detectorEnabled: bool,
  detectorAvailable: bool,
  needsSignIn: bool,
  signingIn: bool,
  captures: list(
    shape({
      id: text,
      state: text,
      reference,
      metadata,
      expected: nullable(text),
      latest: nullable(text),
      has_candidate: bool,
      has_submission: bool,
    }),
  ),
  mutations: list(mutation),
  rejected: list(
    (value) => Array.isArray(value) && value.length === 2 && mutation(value[0]) && text(value[1]),
  ),
  lifecycle: shape({
    roots: list(
      shape({
        identity,
        exposed: bool,
        enrolled: bool,
        verifiedCode: nullable(text),
        pendingReplacement: nullable(shape({ device: text, verifiedCode: text })),
        certificates: list(enrollment),
        removed: list(text),
      }),
    ),
    joins: list(shape({ request: enrollmentRequest, code: text })),
    offers: list(shape({ id: text, proposal, code: text })),
    pending: list(shape({ identity, operation: text })),
    pages,
    removal: nullable(
      shape({
        identity,
        target: text,
        compromised: bool,
        affected: list(text),
        authority_base: number,
      }),
    ),
  }),
  pendingFoundings: list(shape({ vault: text, operation: text })),
  authority: nullable(
    shape({
      identity,
      sequence: number,
      devices: list(shape({ device: text, removed: bool, roles: list(text) })),
    }),
  ),
  cloud: nullable(shape({ api: text, authentication: text })),
  current: nullable(text),
  canFound: bool,
  error: nullable(text),
});
function isVaultState(value: unknown): value is VaultState {
  return state(value);
}
function isEnrollmentRequest(value: unknown): value is EnrollmentRequest {
  return enrollmentRequest(value);
}
function isProposal(value: unknown): value is JoinProposal {
  return proposal(value);
}
export function readVaultState(value: unknown): VaultState {
  if (!isVaultState(value)) {
    throw new Error("The native vault projection is invalid.");
  }
  return value;
}
export function readEnrollmentRequest(source: string): EnrollmentRequest {
  const value: unknown = JSON.parse(source);
  if (!isEnrollmentRequest(value)) {
    throw new Error("Invalid public device request.");
  }
  return value;
}
export function readJoinProposal(source: string): JoinProposal {
  const value: unknown = JSON.parse(source);
  if (!isProposal(value)) {
    throw new Error("Invalid public enrollment proposal.");
  }
  return value;
}
