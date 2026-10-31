import { afterEach, expect, test } from "bun:test";

import { NodeProtocol } from "@swarmie/plugin-client-node-browser";
import type { McpDefinition, McpServer } from "@swarmie/plugin-mcp-browser";
import { orb } from "@syntropika/tembra-sdk";
import { createHost } from "@syntropika/tembra-sdk/node";

import { initializeClient } from "../src/client/initialize";

const cleanup: (() => Promise<void>)[] = [];
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

type BackendState = {
  servers: McpServer[];
  saves: number;
  reads: number;
  save: (definition: McpDefinition) => Promise<void>;
};

async function outcome(operation: Promise<unknown>) {
  try {
    await operation;
    return null;
  } catch (error: unknown) {
    return error;
  }
}

function definition(name = "Documents"): McpDefinition {
  return {
    id: "documents",
    name,
    endpoint: { kind: "http", url: "https://example.com/mcp", headers: {} },
    protocol: "2026-07-28",
    scope: { kind: "user" },
    revision: 1,
    enabled: false,
  };
}

async function backend() {
  const host = createHost();
  const state: BackendState = {
    servers: [],
    saves: 0,
    reads: 0,
    save: async () => undefined,
  };
  await host.install(
    orb({
      name: "fixture.mcp.settings",
      apiVersion: 1,
      services: {
        "swarmie.mcp.settings": {
          create: () => undefined,
          methods: {
            list() {
              state.reads += 1;
              return state.servers;
            },
            async save(_value, _context, input: { definition: McpDefinition }) {
              state.saves += 1;
              await state.save(input.definition);
              state.servers = [
                {
                  definition: input.definition,
                  observation: { status: "disconnected", protocol: null, tools: [] },
                },
              ];
            },
          },
        },
      },
    }),
  );
  cleanup.push(async () => {
    await host.close();
  });
  return { host, state };
}

async function until(predicate: () => boolean, remaining = 100): Promise<void> {
  if (predicate()) {
    return;
  }
  if (remaining === 0) {
    throw new Error("MCP presentation did not settle.");
  }
  await Bun.sleep(5);
  await until(predicate, remaining - 1);
}

async function setup() {
  const a = await backend();
  const b = await backend();
  const nodes = ["a", "b"].map((id) => ({ id, label: id, principal: "alice", url: null }));
  const client = await initializeClient(createHost(), {
    dev: false,
    nodes,
    connector: async (node) => {
      const source = node.id === "a" ? a : b;
      return {
        identity: {
          nodeId: node.id,
          principal: node.principal,
          protocolVersion: NodeProtocol.Version,
        },
        get: async (service, key) => await source.host.get(service, key),
        close: () => undefined,
      };
    },
  });
  cleanup.push(async () => {
    await client.close();
  });
  await until(() => client.mcp.state.getSnapshot().available);
  return { client, a, b };
}

test("a pending settings write is not duplicated and a late acknowledgement cannot populate another node", async () => {
  const { client, a, b } = await setup();
  const gate = Promise.withResolvers<undefined>();
  a.state.save = async () => {
    await gate.promise;
  };
  const first = outcome(client.mcp.save(definition(), null));
  expect(await outcome(client.mcp.save(definition(), null))).toMatchObject({
    message: "Wait for the current MCP action to finish.",
  });
  await until(() => a.state.saves === 1);
  await client.connect("b");
  client.selectNode("b");
  await until(
    () => client.mcp.state.getSnapshot().available && client.mcp.state.getSnapshot().nodeId === "b",
  );
  gate.resolve(undefined);
  expect(await first).toBeInstanceOf(Error);
  expect(a.state.saves).toBe(1);
  expect(b.state.saves).toBe(0);
  expect(client.mcp.state.getSnapshot().servers).toEqual([]);
  await client.mcp.save(definition("B documents"), null);
  expect(client.mcp.state.getSnapshot().servers[0]?.definition.name).toBe("B documents");
});

test("an unknown write result is reconciled by reading without replaying the mutation", async () => {
  const { client, a } = await setup();
  const saved = definition();
  a.state.save = async () => {
    a.state.servers = [
      { definition: saved, observation: { status: "disconnected", protocol: null, tools: [] } },
    ];
    throw new Error("The reply was lost after the write.");
  };
  const before = a.state.reads;
  expect(await outcome(client.mcp.save(saved, null))).toMatchObject({
    message: "The reply was lost after the write.",
  });
  expect(a.state.saves).toBe(1);
  expect(a.state.reads).toBe(before + 1);
  expect(client.mcp.state.getSnapshot().servers[0]?.definition.id).toBe(saved.id);
  expect(client.mcp.state.getSnapshot().error).toContain("The reply was lost");
});
