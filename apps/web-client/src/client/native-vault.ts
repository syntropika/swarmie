import { readVaultState, unavailableVault } from "@swarmie/plugin-vault-browser";
import type { VaultBackend, VaultState } from "@swarmie/plugin-vault-browser";
import type { DynamicReference } from "@syntropika/tembra-sdk";
import { RemoteHost } from "@syntropika/tembra-sdk/rpc";
import type { Transport } from "@syntropika/tembra-sdk/rpc";

/** Native SDK is a private in-process byte carrier; Tembra remains the RPC owner. */
export async function nativeVault(): Promise<VaultBackend | undefined> {
  const zero: unknown = Reflect.get(window, "zero");
  if (zero === null || typeof zero !== "object") {
    return undefined;
  }
  const invoke: unknown = Reflect.get(zero, "invoke");
  if (typeof invoke !== "function") {
    return undefined;
  }
  const native = async (command: string, payload?: unknown): Promise<unknown> =>
    await Reflect.apply(invoke, zero, [command, payload]);
  try {
    if ((await native("swarmie.native.available")) !== true) {
      return undefined;
    }
  } catch {
    return undefined;
  }
  let closed = false;
  let connection: NativeConnection | undefined;
  try {
    connection = await connect(native);
  } catch {
    // The same read-only reconnect loop also owns an interrupted first attachment.
  }
  const listeners = new Set<() => void>();
  let projection: VaultState = connection
    ? readVaultState(connection.reference.state.value)
    : unavailableVault;
  const copy = () => {
    if (closed || connection === undefined) {
      return;
    }
    projection = readVaultState(connection.reference.state.value);
    for (const listener of listeners) {
      listener();
    }
  };
  let detach = connection?.reference.state.subscribe(copy);
  async function call(method: string, ...args: unknown[]): Promise<unknown> {
    // Failed effects are never replayed on reattachment. Read refresh alone reconnects the channel.
    if (closed || connection === undefined || !connection.live()) {
      throw new Error("The private channel is reconnecting.");
    }
    const fn: unknown = Reflect.get(connection.reference, method);
    if (typeof fn !== "function") {
      throw new TypeError("The private capability is unavailable.");
    }
    return await Reflect.apply(fn, undefined, args);
  }
  let refresh: ReturnType<typeof setTimeout> | undefined;
  async function update() {
    try {
      if ((connection === undefined || !connection.live()) && !closed) {
        detach?.();
        connection?.close();
        const replacement = await connect(native);
        if (closed) {
          replacement.close();
          return;
        }
        connection = replacement;
        detach = connection.reference.state.subscribe(copy);
        copy();
      }
      await call("read");
    } catch {
      /* Retain the last safe projection; no operation is retried. */
    }
    if (!closed) {
      refresh = setTimeout(() => {
        void update().catch(() => false);
      }, 2000);
    }
  }
  void update().catch(() => false);
  return {
    state: {
      getSnapshot: () => projection,
      subscribe(changed) {
        listeners.add(changed);
        return () => {
          listeners.delete(changed);
        };
      },
    },
    async signIn(endpoints) {
      const url = await call("login", endpoints);
      if (typeof url !== "string" || new URL(url).protocol !== "https:") {
        throw new Error("Invalid authorization URL.");
      }
      await native("native-sdk.os.openUrl", { url });
    },
    async page(key, offset) {
      await call("page", key, offset);
    },
    async lifecycle(request) {
      await call("lifecycle", request);
    },
    async recovery(request) {
      await call("recovery", request);
    },
    async resolveFounding(vault) {
      await call("resolve_founding", vault);
    },
    async found(expectedCurrent) {
      await call("found", { expected_current: expectedCurrent });
    },
    async signOut() {
      await call("logout");
    },
    async unlock() {
      await call("unlock", null);
    },
    async synchronize() {
      await call("synchronize");
    },
    async save(metadata, value, existing) {
      await call("save", {
        metadata,
        value,
        reference: existing?.reference ?? null,
        revision: existing?.revision ?? null,
      });
    },
    async remove(entry) {
      await call("remove", entry.reference, entry.revision);
    },
    async submitEntry(card, value) {
      await call("submit_entry", card, value);
    },
    async decide(card, choice) {
      await call("decide", card, choice);
    },
    async configureDetector(enabled) {
      await call("detector", enabled);
    },
    close() {
      closed = true;
      clearTimeout(refresh);
      detach?.();
      connection?.close();
      listeners.clear();
    },
  };
}

type NativeInvoke = (command: string, payload?: unknown) => Promise<unknown>;
type NativeConnection = { reference: DynamicReference; live: () => boolean; close: () => void };

/** One byte attachment; Tembra supplies RPC, pending-call failure and reference ownership. */
async function connect(native: NativeInvoke): Promise<NativeConnection> {
  await native("swarmie.native.connect");
  let live = true;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let end: (() => void) | undefined;
  const transport: Transport = {
    async send(frame) {
      if (!live) {
        throw new Error("The private native channel ended.");
      }
      try {
        await native("swarmie.native.send", {
          frame: new TextDecoder("utf-8", { fatal: true }).decode(frame),
        });
      } catch (error) {
        live = false;
        end?.();
        throw error;
      }
    },
    listen(receive, ended) {
      end = () => {
        clearTimeout(timer);
        ended(new Error("The private native channel ended."));
      };
      async function drain(remaining: number): Promise<void> {
        if (!live || remaining === 0) {
          return;
        }
        const frame = await native("swarmie.native.receive");
        if (frame === null) {
          return;
        }
        if (typeof frame !== "string") {
          throw new TypeError("Private channel refused");
        }
        receive(new TextEncoder().encode(frame));
        await drain(remaining - 1);
      }
      async function poll() {
        try {
          await drain(8);
        } catch {
          live = false;
          end?.();
        }
        if (live) {
          timer = setTimeout(() => {
            void poll().catch(() => false);
          }, 16);
        }
      }
      void poll().catch(() => false);
      return () => {
        live = false;
        clearTimeout(timer);
      };
    },
    close() {
      live = false;
      clearTimeout(timer);
    },
  };
  const remote = new RemoteHost(transport, { maxFrame: 400_000, maxPending: 8 });
  try {
    const reference = await remote.get("swarmie.vault.control", "main", {
      signal: AbortSignal.timeout(10_000),
    });
    return {
      reference,
      live: () => live,
      close() {
        reference.release();
        remote.close();
      },
    };
  } catch (error) {
    remote.close();
    throw error;
  }
}
