import type { AccountMethods } from "@swarmie/contracts";

import { decodeAccountResult } from "~/src/account-wire";
import { responseResult } from "~/src/checked-json";
// Copyright 2026 Syntropika
// SPDX-License-Identifier: Apache-2.0
import { clientRequest, nativeClient } from "~/src/client-transport";

let sequence = 0;
/** UUIDv7 binds receipt issuance time so expired writes cannot be replayed after server cleanup. */
export function accountReceiptId(): string {
  const bytes = crypto.getRandomValues(new Uint8Array(16));
  let milliseconds = Date.now();
  for (let index = 5; index >= 0; index += -1) {
    bytes[index] = milliseconds % 256;
    milliseconds = Math.floor(milliseconds / 256);
  }
  bytes[6] = (bytes[6] % 16) + 112;
  bytes[8] = (bytes[8] % 64) + 128;
  const hex = [...bytes].map((value) => value.toString(16).padStart(2, "0")).join("");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}
/** Account commands have no execution-node preflight or selected-node dependency. */
export async function accountRequest<M extends keyof AccountMethods>(
  method: M,
  params: AccountMethods[M]["params"],
): Promise<AccountMethods[M]["result"]> {
  return decodeAccountResult(method, await accountTransport(method, params));
}

/** One raw native I/O boundary is shared by the observer and typed account commands. */
export async function accountTransport(
  method: keyof AccountMethods,
  params: unknown,
): Promise<unknown> {
  if (!nativeClient()) {
    throw new Error("Open the desktop app to manage your account.");
  }
  const response = await clientRequest(
    "/client/rpc",
    { jsonrpc: "2.0", id: (sequence += 1), method, params },
    "",
    60_000,
  );
  if (!response.ok) {
    throw new Error("The account owner is unavailable. Try again from the desktop app.");
  }
  const value: unknown = await response.json();
  return responseResult(value);
}
