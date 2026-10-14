// Copyright 2026 Syntropika
// SPDX-License-Identifier: Apache-2.0
import { clientRequest, nativeClient } from "./clientTransport";
import type { AccountMethods } from "@swarmie/contracts";
let sequence = 0;
/** Account commands have no execution-node preflight or selected-node dependency. */
export async function accountRequest<M extends keyof AccountMethods>(method: M, params: AccountMethods[M]["params"]): Promise<AccountMethods[M]["result"]> {
  if (!nativeClient()) throw new Error("Open the desktop app to manage your account.");
  const response = await clientRequest("/client/rpc", { jsonrpc: "2.0", id: ++sequence, method, params }, "", 60_000);
  if (!response.ok) throw new Error("The account owner is unavailable. Try again from the desktop app.");
  const value = await response.json();
  if (value.error) throw new Error(value.error.message);
  return value.result;
}
