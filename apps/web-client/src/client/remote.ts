import type { DynamicReference } from "@syntropika/tembra-sdk";

/** Invoke a dynamic node method without acquiring authority, validating data or retrying. */
export async function callRemote(
  peer: DynamicReference,
  method: string,
  args: unknown[],
  unavailable: string,
): Promise<unknown> {
  const call: unknown = Reflect.get(peer, method);
  if (typeof call !== "function") {
    throw new TypeError(unavailable);
  }
  return await Reflect.apply(call, undefined, args);
}
