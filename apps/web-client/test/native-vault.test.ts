import { expect, test } from "bun:test";

import { unavailableVault } from "@swarmie/plugin-vault-browser";
import { orb } from "@syntropika/tembra-sdk";
import type { Context, State } from "@syntropika/tembra-sdk";
import { createHost } from "@syntropika/tembra-sdk/node";
import { serve } from "@syntropika/tembra-sdk/rpc";
import type { Transport } from "@syntropika/tembra-sdk/rpc";

import { nativeVault } from "../src/client/native-vault";

async function eventually(condition: () => boolean): Promise<void> {
  const pending = Promise.withResolvers<boolean>();
  const interval = setInterval(() => {
    if (condition()) {
      pending.resolve(true);
    }
  }, 10);
  const timeout = setTimeout(() => {
    pending.reject(new Error("Fixture did not settle"));
  }, 8000);
  await pending.promise.finally(() => {
    clearInterval(interval);
    clearTimeout(timeout);
  });
}

test("initial private attachment retries and reattachment never repeats a save whose reply was lost", async () => {
  const host = createHost();
  let writes = 0;
  let connections = 0;
  let loseReplies = false;
  let projection = { ...unavailableVault, phase: "signed-out" as const };
  let incoming: ((frame: Uint8Array) => void) | undefined;
  let server: { close: () => void } | undefined;
  const frames: string[] = [];
  await host.install(
    orb({
      name: "swarmie.vault.control",
      apiVersion: 1,
      services: {
        "swarmie.vault.control": {
          create: (self) => self.state(projection),
          methods: {
            read(state: State<typeof projection>, context: Context) {
              return state.update(context, () => projection);
            },
            save(state: State<typeof projection>, context: Context, entry: unknown) {
              expect(typeof entry).toBe("object");
              writes += 1;
              projection = { ...projection, detectorAvailable: true };
              loseReplies = true;
              return state.update(context, () => projection);
            },
          },
        },
      },
    }),
  );
  await host.install(
    orb({
      name: "swarmie.providers.protection",
      apiVersion: 1,
      services: {
        "swarmie.providers.protection": {
          create: (self) =>
            self.state({ cards: [], detectorEnabled: true, detectorAvailable: false }),
          methods: {
            read(
              state: State<{
                cards: never[];
                detectorEnabled: boolean;
                detectorAvailable: boolean;
              }>,
              context: Context,
            ) {
              state.update(context, () => ({
                cards: [],
                detectorEnabled: true,
                detectorAvailable: projection.detectorAvailable,
              }));
              const revision = String(projection.detectorAvailable);
              return {
                revision,
                data: JSON.stringify({
                  revision,
                  cards: [],
                  detectorEnabled: true,
                  detectorAvailable: projection.detectorAvailable,
                }),
                next: null,
              };
            },
          },
        },
      },
    }),
  );
  const original = Object.getOwnPropertyDescriptor(globalThis, "window");
  Object.defineProperty(globalThis, "window", {
    configurable: true,
    value: {
      zero: {
        async invoke(command: string, payload?: unknown): Promise<unknown> {
          if (command === "swarmie.native.available") {
            return true;
          }
          if (command === "swarmie.native.connect") {
            connections += 1;
            if (connections === 1) {
              throw new Error("Synthetic initial attachment interruption");
            }
            server?.close();
            frames.length = 0;
            loseReplies = false;
            const transport: Transport = {
              async send(frame) {
                if (!loseReplies) {
                  frames.push(new TextDecoder().decode(frame));
                }
              },
              listen(receive) {
                incoming = receive;
                return () => {
                  incoming = undefined;
                };
              },
              close() {
                incoming = undefined;
              },
            };
            server = serve(host, transport, { principal: "desktop-shell", maxFrame: 400_000 });
            return null;
          }
          if (command === "swarmie.native.receive") {
            return loseReplies ? false : (frames.shift() ?? null);
          }
          if (command === "swarmie.native.send") {
            const frame: unknown =
              payload !== null && typeof payload === "object"
                ? Reflect.get(payload, "frame")
                : undefined;
            if (typeof frame !== "string" || !incoming) {
              throw new Error("Fixture attachment unavailable");
            }
            incoming(new TextEncoder().encode(frame));
            return null;
          }
          throw new Error("Unexpected native operation");
        },
      },
    },
  });
  const backend = await nativeVault();
  try {
    expect(backend).toBeDefined();
    if (!backend) {
      throw new Error("Missing private backend");
    }
    await eventually(() => backend.state.getSnapshot().phase === "signed-out");
    let refused = false;
    try {
      await backend.save(
        {
          name: "synthetic",
          purpose: "",
          projects: { kind: "unrestricted" },
          agents: { kind: "unrestricted" },
          approval: "none",
          policy_generation: crypto.randomUUID(),
          target_account: null,
        },
        "synthetic-input",
      );
    } catch {
      refused = true;
    }
    expect(refused).toBe(true);
    await eventually(() => connections >= 3 && backend.state.getSnapshot().detectorAvailable);
    expect(writes).toBe(1);
    expect(backend.state.getSnapshot().detectorAvailable).toBe(true);
  } finally {
    backend?.close();
    server?.close();
    await host.close();
    if (original) {
      Object.defineProperty(globalThis, "window", original);
    } else {
      Reflect.deleteProperty(globalThis, "window");
    }
  }
}, 12_000);
