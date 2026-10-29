import type { GitConnection } from "./remote";

const inMemory = new Map<string, string | null>();

export function pendingKey(connection: GitConnection | null, path: string) {
  return JSON.stringify(["swarmie.git.pending", connection?.nodeId, connection?.principal, path]);
}

/** Persist only opaque operation identifiers; credentials and file contents stay on the node. */
export function readPending(key: string) {
  if (inMemory.has(key)) {
    return inMemory.get(key) ?? null;
  }
  if (typeof sessionStorage === "undefined") {
    return inMemory.get(key) ?? null;
  }
  try {
    return sessionStorage.getItem(key);
  } catch {
    return inMemory.get(key) ?? null;
  }
}

export function writePending(key: string, value: string | null) {
  inMemory.set(key, value);
  try {
    if (typeof sessionStorage === "undefined") {
      return;
    }
    if (value === null) {
      sessionStorage.removeItem(key);
    } else {
      sessionStorage.setItem(key, value);
    }
  } catch {
    // The native operation catalog remains authoritative when browser storage is blocked.
  }
}
