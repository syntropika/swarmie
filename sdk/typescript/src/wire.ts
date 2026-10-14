import { ProtocolError } from "./protocol-error.js";
import { RpcError } from "./rpc-error.js";

export function positive(value: number, label: string) {
  if (!Number.isFinite(value) || value <= 0) {
    throw new RangeError(`${label} must be positive`);
  }
  return value;
}
export function decodeResponse(value: unknown, id: string): unknown {
  if (typeof value !== "object" || value === null) {
    throw new ProtocolError("Invalid JSON-RPC response");
  }
  if (!("jsonrpc" in value) || value.jsonrpc !== "2.0" || !("id" in value) || value.id !== id) {
    throw new ProtocolError("JSON-RPC response identity mismatch");
  }
  if ("error" in value) {
    if (
      "result" in value ||
      typeof value.error !== "object" ||
      value.error === null ||
      !("code" in value.error) ||
      typeof value.error.code !== "number" ||
      !Number.isSafeInteger(value.error.code) ||
      !("message" in value.error) ||
      typeof value.error.message !== "string"
    ) {
      throw new ProtocolError("Malformed JSON-RPC error");
    }
    throw new RpcError(
      value.error.code,
      value.error.message,
      "data" in value.error ? value.error.data : undefined,
    );
  }
  if (!("result" in value)) {
    throw new ProtocolError("JSON-RPC response has no result");
  }
  return value.result;
}

// Protocol DTOs come from the daemon's canonical schema. This conversion belongs
// at the typed transport boundary; callRaw remains available for untyped values.
// oxlint-disable-next-line typescript/no-unnecessary-type-parameters -- The schema result type is supplied by the owning RPC method.
export function fromWire<T>(value: unknown): T {
  // SAFETY: the SDK associates this result with its schema-selected RPC method.
  // oxlint-disable-next-line typescript/no-unsafe-type-assertion -- Schema DTOs have no JavaScript representation; transport validates the envelope.
  return value as T;
}
