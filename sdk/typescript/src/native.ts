import { createRequire } from "node:module";

import { ProtocolError } from "./protocol-error.js";
import { RpcError } from "./rpc-error.js";

type Binding = {
  control: (command: string) => string;
  execute: (command: string) => Promise<string>;
};
function isBinding(value: unknown): value is Binding {
  return (
    typeof value === "object" &&
    value !== null &&
    "control" in value &&
    typeof value.control === "function" &&
    "execute" in value &&
    typeof value.execute === "function"
  );
}
const require = createRequire(import.meta.url);
// The Node-API module is built and staged with this package for its host platform.
const candidate: unknown = require("../native/swarmie_client.node");
if (!isBinding(candidate)) {
  throw new ProtocolError("Invalid native client binding");
}
const binding = candidate;

function unwrap(encoded: string): unknown {
  const value: unknown = JSON.parse(encoded);
  if (typeof value !== "object" || value === null) {
    throw new ProtocolError("Invalid native reply");
  }
  if ("ok" in value) {
    return value.ok;
  }
  if (!("error" in value) || typeof value.error !== "object" || value.error === null) {
    throw new ProtocolError("Invalid native error");
  }
  const { error } = value;
  if (!("message" in error) || typeof error.message !== "string" || !("kind" in error)) {
    throw new ProtocolError("Invalid native error");
  }
  if (error.kind === "rpc" && "code" in error && typeof error.code === "number") {
    const data: unknown =
      "data_json" in error && typeof error.data_json === "string"
        ? JSON.parse(error.data_json)
        : undefined;
    throw new RpcError(error.code, error.message, data);
  }
  if (error.kind === "cancelled") {
    throw new DOMException(error.message, "AbortError");
  }
  throw new ProtocolError(error.message);
}
export function control(command: object): unknown {
  return unwrap(binding.control(JSON.stringify(command)));
}
export function handle(command: object): number {
  const value = control(command);
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value <= 0) {
    throw new ProtocolError("Invalid native handle");
  }
  return value;
}
export function jsonResult(value: unknown): unknown {
  if (typeof value !== "string") {
    throw new ProtocolError("Invalid native JSON result");
  }
  return JSON.parse(value);
}
export async function execute(command: object, signal?: AbortSignal): Promise<unknown> {
  const operation = handle({ action: "prepare" });
  const cancel = () => {
    binding.control(JSON.stringify({ action: "cancel", operation }));
  };
  signal?.addEventListener("abort", cancel, { once: true });
  try {
    if (signal?.aborted === true) {
      cancel();
    }
    return unwrap(await binding.execute(JSON.stringify({ ...command, operation })));
  } finally {
    signal?.removeEventListener("abort", cancel);
    control({ action: "release", handle: operation });
  }
}
const released = new FinalizationRegistry<number>((id) => {
  binding.control(JSON.stringify({ action: "release", handle: id }));
});
export function own(target: object, id: number): void {
  released.register(target, id, target);
}
export function release(target: object, id: number): void {
  released.unregister(target);
  control({ action: "release", handle: id });
}
