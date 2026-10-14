// Copyright 2026 Syntropika
// SPDX-License-Identifier: Apache-2.0
import { clientRequest, nativeClient } from "./clientTransport";
import type { AccountMethods } from "@swarmie/contracts";
let sequence = 0;
/** UUIDv7 binds receipt issuance time so expired writes cannot be replayed after server cleanup. */
export function accountReceiptId(): string {
  const bytes = crypto.getRandomValues(new Uint8Array(16));
  let milliseconds = Date.now();
  for (let index = 5; index >= 0; index--) { bytes[index] = milliseconds % 256; milliseconds = Math.floor(milliseconds / 256); }
  bytes[6] = (bytes[6] & 0x0f) | 0x70;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;
  const hex = [...bytes].map((value) => value.toString(16).padStart(2, "0")).join("");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}
/** Account commands have no execution-node preflight or selected-node dependency. */
export async function accountRequest<M extends keyof AccountMethods>(method: M, params: AccountMethods[M]["params"]): Promise<AccountMethods[M]["result"]> {
  if (!nativeClient()) throw new Error("Open the desktop app to manage your account.");
  const response = await clientRequest("/client/rpc", { jsonrpc: "2.0", id: ++sequence, method, params }, "", 60_000);
  if (!response.ok) throw new Error("The account owner is unavailable. Try again from the desktop app.");
  const value = await response.json();
  if (value.error) throw new Error(value.error.message);
  return value.result;
}
