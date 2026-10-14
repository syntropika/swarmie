// Copyright 2026 Syntropika
// SPDX-License-Identifier: Apache-2.0
/** Sanitized account state from the persistent per-user native owner. */
export type AccountProfile = { id: string; displayName: string | null; email: string; emailVerified: boolean; avatarUrl: string | null; revision: number };
export type AccountSnapshot = { status: "signedIn" | "signedOut" | "signingIn" | "storageUnavailable"; generation: number; profile: AccountProfile | null; persistence: "secureVault" | "session"; loginId: string | null; authorizationUrl?: string | null; configured?: boolean };
/** Owner incarnation and publication revision also reconcile after restart or service reconfiguration. */
export type AccountCursor = { ownerId: string; revision: number };
export type AccountObservation = { cursor: AccountCursor; account: AccountSnapshot };
/** ISO 8601 timestamps; lastUsedAt updates at most once per five minutes or on renewal. */
export type AccountSession = { id: string; installationId: string; authenticatedAt: string; createdAt: string; lastUsedAt: string; expiresAt: string };
export type AccountSessionPage = { current: AccountSession; others: AccountSession[]; nextCursor: string | null };
/** Profile mutation requestId values are UUIDv7; their issuance time bounds receipt retry validity. */
export interface AccountMethods {
  "account/read": { params: Record<string, never>; result: AccountSnapshot };
  "account/observe": { params: { cursor?: AccountCursor }; result: AccountObservation };
  "account/configure": { params: { origin: string }; result: AccountSnapshot };
  "account/login/start": { params: { sessionOnly?: boolean; openBrowser?: boolean }; result: { loginId: string; authorizationUrl: string; expiresIn: number; persistence: "secureVault" | "session"; browserOpened?: boolean } };
  "account/login/cancel": { params: { loginId: string }; result: AccountSnapshot };
  "account/logout": { params: { expectedAccountId?: string }; result: { account: AccountSnapshot; remoteRevocationConfirmed: boolean } };
  "account/sessions/read": { params: { expectedAccountId: string; after?: string }; result: AccountSessionPage };
  "account/sessions/revoke": { params: { expectedAccountId: string; sessionId: string }; result: AccountSnapshot };
  "account/sessions/revoke-all": { params: { expectedAccountId: string }; result: AccountSnapshot };
  "account/profile/read": { params: Record<string, never>; result: AccountSnapshot };
  "account/profile/avatar/update": { params: { accountId: string; imageBase64: string | null; expectedRevision: number; requestId: string }; result: AccountSnapshot };
  "account/profile/update": { params: { accountId: string; displayName: string | null; expectedRevision: number; requestId: string }; result: AccountSnapshot };
}
