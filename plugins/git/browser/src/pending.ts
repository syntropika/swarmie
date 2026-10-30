import { targetValue } from "./protocol";
import type { GitRequest, RepositoryTarget } from "./protocol";
import type { GitConnection } from "./remote";

type Pending = { operation_id: string; target: RepositoryTarget | null };
const inMemory = new Map<string, Pending | null>();

export function pendingKey(connection: GitConnection | null, path: string) {
  return JSON.stringify(["swarmie.git.pending", connection?.nodeId, connection?.principal, path]);
}

/** Keep the ID and captured checkout only. File contents and operation arguments stay on the node. */
function read(key: string): Pending | null {
  if (inMemory.has(key)) {
    return inMemory.get(key) ?? null;
  }
  let stored: string | null;
  try {
    stored = typeof sessionStorage === "undefined" ? null : sessionStorage.getItem(key);
  } catch {
    return inMemory.get(key) ?? null;
  }
  if (stored === null) {
    return null;
  }
  // Retain existing user markers. An opaque legacy ID still uses explicit recovery.
  if (!stored.startsWith("{")) {
    return { operation_id: stored, target: null };
  }
  const parsed: unknown = JSON.parse(stored);
  if (
    typeof parsed !== "object" ||
    parsed === null ||
    !("operation_id" in parsed) ||
    typeof parsed.operation_id !== "string"
  ) {
    throw new Error("The pending Git identity is invalid; retain it for recovery.");
  }
  return {
    operation_id: parsed.operation_id,
    target: "target" in parsed && parsed.target !== null ? targetValue(parsed.target) : null,
  };
}

export function readPending(key: string): string | null {
  return read(key)?.operation_id ?? null;
}

export function pendingRecovery(key: string, operationId: string): GitRequest {
  const pending = read(key);
  if (pending?.operation_id !== operationId) {
    throw new Error("The pending Git operation changed before reconciliation.");
  }
  return pending.target === null
    ? { kind: "recover", operation_id: operationId }
    : { kind: "reconcile", operation_id: operationId, target: pending.target };
}

export function writePending(key: string, value: string | null, target?: RepositoryTarget) {
  const previous = read(key);
  const pending =
    value === null
      ? null
      : {
          operation_id: value,
          target: (previous?.operation_id === value ? previous.target : null) ?? target ?? null,
        };
  inMemory.set(key, pending === null ? null : structuredClone(pending));
  try {
    if (typeof sessionStorage === "undefined") {
      return;
    }
    if (pending === null) {
      sessionStorage.removeItem(key);
    } else {
      sessionStorage.setItem(key, JSON.stringify(pending));
    }
  } catch {
    // The native operation catalog remains authoritative when browser storage is blocked.
  }
}
