import type { NodeConnection } from "@swarmie/plugin-browser/connections";
import type {
  ProviderAuthorizationSession,
  ProviderAuthorizationState,
} from "@swarmie/plugin-providers-browser";

import { randomId } from "../id";
import { providerAccount } from "./provider-values";
import { callRemote } from "./remote";

const unavailable = "The node authorization service is unavailable.";

function nullableText(value: unknown): value is string | null {
  return value === null || typeof value === "string";
}
function detached() {
  return undefined;
}
function authorization(value: unknown): ProviderAuthorizationState {
  if (typeof value !== "object" || value === null || !("kind" in value)) {
    throw new Error("The authorization session is unavailable.");
  }
  if (value.kind === "connected" && "account" in value) {
    return { kind: "connected", account: providerAccount(value.account) };
  }
  if (value.kind === "failed" && "error" in value && typeof value.error === "string") {
    return {
      kind: "failed",
      error: value.error,
      retryable: "retryable" in value && value.retryable === true,
    };
  }
  if (
    value.kind !== "waiting" ||
    !("authorizationUrl" in value) ||
    typeof value.authorizationUrl !== "string" ||
    !("userCode" in value) ||
    !nullableText(value.userCode) ||
    !("callbackUri" in value) ||
    !nullableText(value.callbackUri)
  ) {
    throw new Error("The authorization instructions are invalid.");
  }
  const url = new URL(value.authorizationUrl);
  if (url.protocol !== "https:" || url.username !== "" || url.password !== "") {
    throw new Error("The provider sign-in URL is invalid.");
  }
  return {
    kind: "waiting",
    authorizationUrl: value.authorizationUrl,
    userCode: value.userCode,
    callbackUri: value.callbackUri,
  };
}

/** The remote Tembra State owns public instructions; tokens stay on the native node. */
export async function openProviderAuthorization(
  connection: NodeConnection,
  providerId: string,
  signal: AbortSignal,
  current: () => boolean,
  accountId?: string,
): Promise<ProviderAuthorizationSession> {
  signal.throwIfAborted();
  const key = [providerId, randomId()];
  if (accountId !== undefined) {
    key.push(accountId);
  }
  const peer = await connection.get("swarmie.providers.authorization", key, {
    signal,
  });
  let disposed = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let previous: unknown;
  let projected: ProviderAuthorizationState;
  let detachPeer: () => void = detached;
  const listeners = new Set<() => void>();
  const ended: ProviderAuthorizationState = {
    kind: "failed",
    error: "The node session changed. Begin a new sign-in.",
  };
  function check() {
    signal.throwIfAborted();
    if (disposed || !connection.isCurrent() || !current()) {
      throw new Error("The authorization belongs to an earlier node session.");
    }
  }
  function dispose() {
    if (disposed) {
      return;
    }
    disposed = true;
    clearTimeout(timer);
    signal.removeEventListener("abort", dispose);
    detachPeer();
    peer.release();
    for (const changed of listeners) {
      changed();
    }
    listeners.clear();
  }
  function snapshot(): ProviderAuthorizationState {
    if (disposed || !connection.isCurrent() || !current()) {
      return ended;
    }
    try {
      const value: unknown = peer.state.value;
      if (value !== previous || projected === undefined) {
        const next = authorization(value);
        previous = value;
        projected = next;
      }
      return projected;
    } catch {
      return ended;
    }
  }
  async function poll() {
    try {
      check();
      await callRemote(peer, "poll", [], unavailable);
      check();
      if (snapshot().kind === "waiting") {
        timer = setTimeout(() => {
          poll().catch(() => undefined);
        }, 1000);
      }
    } catch {
      dispose();
    }
  }
  try {
    signal.addEventListener("abort", dispose, { once: true });
    check();
    await callRemote(peer, "poll", [], unavailable);
    check();
    authorization(peer.state.value);
    detachPeer = peer.state.subscribe(() => {
      if (snapshot().kind !== "waiting") {
        clearTimeout(timer);
      }
      for (const changed of listeners) {
        changed();
      }
    });
    if (snapshot().kind === "waiting") {
      timer = setTimeout(() => {
        poll().catch(() => undefined);
      }, 1000);
    }
    return {
      getSnapshot: snapshot,
      subscribe(changed) {
        listeners.add(changed);
        return () => {
          listeners.delete(changed);
        };
      },
      async complete(callbackUrl) {
        check();
        await callRemote(peer, "complete", [callbackUrl], unavailable);
        check();
      },
      async cancel() {
        check();
        if (snapshot().kind === "connected") {
          return;
        }
        await callRemote(peer, "cancel", [], unavailable);
      },
      async retry() {
        check();
        await callRemote(peer, "retry", [], unavailable);
        check();
      },
      dispose,
    };
  } catch (error) {
    dispose();
    throw error;
  }
}
