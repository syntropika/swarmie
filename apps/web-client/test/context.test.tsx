import { afterEach, expect, test } from "bun:test";

import { NodeProtocol } from "@swarmie/plugin-browser/connections";
import { ContextPanel } from "@swarmie/plugin-context-browser/views";
import { orb } from "@syntropika/tembra-sdk";
import type { State, Context } from "@syntropika/tembra-sdk";
import { createHost } from "@syntropika/tembra-sdk/node";
import { renderToStaticMarkup } from "react-dom/server";

import { initializeClient } from "../src/client/initialize";
import { executeSlashCommand } from "../src/conversations/composer/actions";

const cleanup: (() => Promise<void>)[] = [];
test("command confirmation clears only its original draft and preserves a newer edit or refusal", async () => {
  const host = createHost();
  const client = await initializeClient(host, { dev: false });
  cleanup.push(client.close);
  const chat = client.activeConversation();
  const action = { execute: () => undefined };
  chat.draft.update((draft) => ({ ...draft, text: "/compact" }));
  await until(() => chat.draft.getSnapshot().text === "/compact");
  const confirmed = Promise.withResolvers<boolean>();
  const command = executeSlashCommand(
    chat,
    "/compact",
    action,
    async () => await confirmed.promise,
  );
  chat.draft.update((draft) => ({ ...draft, text: "A newer draft" }));
  await until(() => chat.draft.getSnapshot().text === "A newer draft");
  confirmed.resolve(true);
  await command;
  await Bun.sleep(5);
  expect(chat.draft.getSnapshot().text).toBe("A newer draft");
  chat.draft.update((draft) => ({ ...draft, text: "/compact" }));
  await until(() => chat.draft.getSnapshot().text === "/compact");
  await executeSlashCommand(chat, "/compact", action, async () => false);
  expect(chat.draft.getSnapshot().text).toBe("/compact");
  await executeSlashCommand(chat, "/compact", action, async () => true);
  await until(() => chat.draft.getSnapshot().text === "");
});
afterEach(async () => {
  await Promise.all(
    cleanup
      .splice(0)
      .toReversed()
      .map(async (close) => {
        await close();
      }),
  );
});
async function outcome(operation: Promise<unknown>) {
  try {
    await operation;
    return null;
  } catch (error: unknown) {
    return error;
  }
}
async function until(predicate: () => boolean, remaining = 100): Promise<void> {
  if (predicate()) {
    return;
  }
  if (remaining === 0) {
    throw new Error("Context presentation did not settle.");
  }
  await Bun.sleep(5);
  await until(predicate, remaining - 1);
}

const initialFence = "12345678-1234-4234-8234-123456789abc";
function snapshot(fence = initialFence) {
  return {
    checkpoint: { active: null, compaction: null },
    fence,
    inspection: {
      stale: true,
      prepared: {
        output_reserve: 20,
        output_reservation: { kind: "sent_cap" },
        model: {
          identity: { provider: "fixture", model: "example-model" },
          generation: "a".repeat(64),
          native_output_maximum: { status: "unknown", reason: "missing" },
          metadata_freshness: {
            prepared_at_ms: "18446744073709551615",
            valid_until_ms: null,
            stale_limit_evidence: 1,
          },
        },
        measurement: {
          input_tokens: 50,
          final_payload_bytes: 200,
          precision: "estimated",
          method: "fixture-estimate",
          components: [
            { kind: "conversation", tokens: 40 },
            { kind: "tools", tokens: 10 },
          ],
        },
      },
      decision: {
        kind: "ready",
        budget: {
          used: 50,
          context: 100,
          input_limit: 70,
          output_reserve: 20,
          headroom: 10,
          available: 20,
          excess: 0,
          precision: "estimated",
        },
      },
    },
    can_compact: true,
    unavailable: null,
  };
}

async function setup() {
  const backend = createHost();
  const calls: {
    command: string;
    operation: string;
    key: readonly string[];
    arguments: unknown;
  }[] = [];
  const effects = new Set<string>();
  const settled = new Set<string>();
  let lose = false;
  let refuse = false;
  let wait: Promise<undefined> = Promise.resolve(undefined);
  let currentFence = initialFence;
  await backend.install(
    orb({
      name: "fixture.context",
      apiVersion: 1,
      services: {
        "swarmie.context": {
          create: (self) => self.state(snapshot(currentFence)),
          methods: {
            inspect: (state: State<ReturnType<typeof snapshot>>, context: Context) => {
              state.update(context, () => snapshot(currentFence));
              return state.value;
            },
          },
        },
        "swarmie.commands": {
          create: (self) => self.key,
          methods: {
            list: () => [{ id: "swarmie.context.compact", aliases: ["/squeeze"] }],
            async invoke(
              key: readonly string[],
              _context: Context,
              input: { command: string; operation: string; arguments: unknown },
            ) {
              calls.push({ ...input, key });
              if (refuse) {
                refuse = false;
                return {
                  requested: input.operation,
                  operation: input.operation,
                  admission: { kind: "refused", reason: "Synthetic local refusal" },
                  snapshot: snapshot(currentFence),
                };
              }
              effects.add(input.operation);
              await wait;
              settled.add(input.operation);
              if (lose) {
                lose = false;
                throw new Error("Synthetic lost receipt");
              }
              return {
                requested: input.operation,
                operation: input.operation,
                admission: { kind: "recognized" },
                snapshot: snapshot(currentFence),
              };
            },
          },
        },
      },
    }),
  );
  cleanup.push(async () => {
    await backend.close();
  });
  const client = await initializeClient(createHost(), {
    dev: false,
    nodes: [{ id: "fixture", label: "Fixture", principal: "alice", url: null }],
    connector: async (node) => ({
      identity: {
        nodeId: node.id,
        principal: node.principal,
        protocolVersion: NodeProtocol.Version,
      },
      get: async (service, key) => await backend.get(service, key),
      close: () => undefined,
    }),
  });
  cleanup.push(client.close);
  await until(() => client.context.state.getSnapshot().phase === "ready");
  return {
    client,
    calls,
    effects,
    settled,
    loseNext: () => {
      lose = true;
    },
    refuseNext: () => {
      refuse = true;
    },
    gate: (value: Promise<undefined>) => {
      wait = value;
    },
    retireFence: () => {
      currentFence = "87654321-4321-4321-8321-cba987654321";
    },
  };
}

test("button and discovered slash alias invoke the stable command without appending chat", async () => {
  const { client, calls, effects } = await setup();
  const chat = client.activeConversation();
  chat.draft.update((draft) => ({ ...draft, text: "Keep my draft" }));
  await until(() => chat.draft.getSnapshot().text === "Keep my draft");
  await client.context.compact();
  const command = client.conversationsUi.composer.slashCommands
    .getSnapshot()
    .find((entry) => entry.id === "swarmie.context.compact");
  expect(command?.command).toBe("squeeze");
  await command?.execute({
    conversationId: chat.id,
    nodeId: "fixture",
    signal: new AbortController().signal,
    insertText: () => {
      throw new Error("A command must not insert model text");
    },
  });
  expect(calls.map((entry) => entry.command)).toEqual([
    "swarmie.context.compact",
    "swarmie.context.compact",
  ]);
  expect(effects.size).toBe(2);
  expect(chat.messages.getSnapshot()).toEqual([]);
  expect(chat.draft.getSnapshot().text).toBe("Keep my draft");
  const markup = renderToStaticMarkup(<ContextPanel api={client.context} />);
  expect(markup).toContain('aria-expanded="true"');
  expect(markup).toContain("Context · ≈ 71% · Previous");
  expect(markup).toContain("Estimated");
  expect(markup).toContain("Out of date");
  expect(markup).toContain("Output reserved");
  expect(markup).toContain("Output bound");
  expect(markup).toContain("Request cap");
  expect(markup).toContain("Tools, including MCP");
  expect(markup).toContain("Metadata prepared");
  expect(markup).toContain("Outside the calendar range");
  expect(markup).toContain("No finite snapshot expiry");
  expect(markup).toContain("Stale limit evidence admitted");
});

test("unknown command receipt keeps one identity until an explicit recognition check", async () => {
  const { client, calls, effects, loseNext, retireFence } = await setup();
  const original = client.activeConversation();
  loseNext();
  expect(await outcome(client.context.compact())).toMatchObject({
    message: "Synthetic lost receipt",
  });
  await until(() => client.context.state.getSnapshot().pending.length === 1);
  await Bun.sleep(20);
  expect(calls).toHaveLength(1);
  expect(renderToStaticMarkup(<ContextPanel api={client.context} />)).toContain(
    "Check compaction request",
  );
  retireFence();
  const other = client.createConversation("fixture");
  await until(
    () =>
      client.context.state.getSnapshot().phase === "ready" &&
      client.context.state.getSnapshot().selection?.conversationId === other.id,
  );
  client.openConversation(original.id);
  await until(
    () =>
      client.context.state.getSnapshot().phase === "ready" &&
      client.context.state.getSnapshot().selection?.conversationId === original.id,
  );
  await client.context.compact();
  await until(() => client.context.state.getSnapshot().pending.length === 0);
  expect(calls).toHaveLength(2);
  expect(calls[0]?.operation).toBe(calls[1]?.operation);
  expect(effects.size).toBe(1);
  expect(calls[1]?.arguments).toEqual({ fence: initialFence });
});

test("a late receipt cannot populate another conversation and observer release does not own effects", async () => {
  const { client, calls, effects, settled, gate } = await setup();
  const settle = Promise.withResolvers<undefined>();
  gate(settle.promise);
  const pending = outcome(client.context.compact());
  await until(() => calls.length === 1);
  const next = client.createConversation("fixture");
  await until(
    () =>
      client.context.state.getSnapshot().phase === "ready" &&
      client.context.state.getSnapshot().selection?.conversationId === next.id,
  );
  expect(settled.size).toBe(0);
  settle.resolve(undefined);
  expect(await pending).toBeInstanceOf(Error);
  expect(client.context.state.getSnapshot().selection?.conversationId).toBe(next.id);
  expect(client.context.state.getSnapshot().view?.operation).toBeNull();
  expect(effects.size).toBe(1);
  expect(settled.size).toBe(1);
  client.context.close();
  expect(
    client.conversationsUi.composer.slashCommands
      .getSnapshot()
      .some((entry) => entry.id === "swarmie.context.compact"),
  ).toBe(false);
  expect(effects.size).toBe(1);
});

test("a confirmed refusal releases only its request and concurrent clicks cannot duplicate admission", async () => {
  const { client, calls, effects, refuseNext, gate } = await setup();
  refuseNext();
  expect(await outcome(client.context.compact())).toMatchObject({
    message: "Synthetic local refusal",
  });
  await until(() => client.context.state.getSnapshot().pending.length === 0);
  expect(client.context.state.getSnapshot().error).toBe("Synthetic local refusal");
  expect(effects.size).toBe(0);
  const settle = Promise.withResolvers<undefined>();
  gate(settle.promise);
  const first = outcome(client.context.compact());
  await until(() => calls.length === 2);
  expect(await outcome(client.context.compact())).toBeInstanceOf(Error);
  expect(calls).toHaveLength(2);
  settle.resolve(undefined);
  expect(await first).toBeNull();
  expect(effects.size).toBe(1);
});
