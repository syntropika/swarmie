import type { GitConnection } from "./remote";

const inMemory = new Map<string, string>();

export function pendingKey(connection: GitConnection | null, path: string) {
  return JSON.stringify(["swarmie.git.pending", connection?.nodeId, connection?.principal, path]);
}

/** Persist only opaque operation identifiers; credentials and file contents stay on the node. */
export function readPending(key: string) {
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
  if (typeof sessionStorage === "undefined") {
    if (value === null) {
      inMemory.delete(key);
    } else {
      inMemory.set(key, value);
    }
  } else if (value === null) {
    sessionStorage.removeItem(key);
  } else {
    sessionStorage.setItem(key, value);
  }
}
