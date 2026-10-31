import { afterEach, expect, test } from "bun:test";

import type { UiStore } from "@swarmie/client-ui";
import { ConnectionPhase, NodeConnection } from "@swarmie/plugin-client-node-browser";
import type { ConnectionsState } from "@swarmie/plugin-client-node-browser";
import { orb } from "@syntropika/tembra-sdk";
import { createHost } from "@syntropika/tembra-sdk/node";

import { createGitPresentation, gitIntegrationOrb } from "../src/presentation";
import type { GitBrowserState } from "../src/presentation";
import { gitRequestKinds } from "../src/remote";

function store<T>(initial: T): UiStore<T> {
  let value = initial;
  const subscribers = new Set<() => void>();
  function set(next: T) {
    value = next;
    for (const subscriber of subscribers) {
      subscriber();
    }
  }
  return {
    getSnapshot: () => value,
    subscribe: (subscriber) => {
      subscribers.add(subscriber);
      return () => {
        subscribers.delete(subscriber);
      };
    },
    set,
    update: (change) => {
      set(change(value));
    },
  };
}
const cleanup: (() => Promise<void>)[] = [];
afterEach(async () => {
  await Promise.all(
    cleanup.splice(0).map(async (close) => {
      await close();
    }),
  );
});

async function fixture(response: () => unknown) {
  const host = createHost();
  await host.install(
    orb({
      name: "fixture.git",
      apiVersion: 1,
      services: { "swarmie.git": { create: () => undefined, methods: { request: response } } },
    }),
  );
  cleanup.push(async () => {
    await host.close();
  });
  const connections = store<Readonly<ConnectionsState>>({
    nodes: [{ id: "node", label: "Node", url: "wss://fixture.invalid", principal: "alice" }],
    selected: "node",
    sessions: {
      node: { nodeId: "node", phase: ConnectionPhase.Connected, generation: 1, failure: null },
    },
  });
  const state = store<Readonly<GitBrowserState>>({ enabled: false, nodes: {} });
  let acquired = 0;
  const presentation = createGitPresentation(state, connections, {
    open: async () => {
      acquired += 1;
      return new NodeConnection(
        "node",
        "alice",
        1,
        {
          identity: { nodeId: "node", principal: "alice", protocolVersion: 1 },
          get: async (service: string, key: string | readonly string[]) =>
            await host.get(service, key),
          close: () => undefined,
        },
        () => connections.getSnapshot().sessions.node?.phase === ConnectionPhase.Connected,
      );
    },
  });
  return { host, state, connections, presentation, acquired: () => acquired };
}
const complete = () => ({
  kind: "capability_info",
  api_version: 3,
  execution_available: true,
  operations: gitRequestKinds,
});

async function settle() {
  await Bun.sleep(20);
}

test("Git availability requires the complete observed interface and current execution support", async () => {
  await Promise.all(
    [
      complete,
      () => ({ ...complete(), execution_available: false }),
      () => ({ ...complete(), api_version: 1 }),
      () => ({ ...complete(), operations: ["discover"] }),
    ].map(async (response) => {
      const value = await fixture(response);
      const dispose = value.presentation.activate();
      await settle();
      expect(value.state.getSnapshot().nodes.node?.available).toBe(response === complete);
      dispose();
      expect(value.state.getSnapshot()).toEqual({ enabled: false, nodes: {} });
    }),
  );
});

test("late capability responses cannot restore availability after connection loss", async () => {
  const delayed = Promise.withResolvers<unknown>();
  const value = await fixture(async () => await delayed.promise);
  const dispose = value.presentation.activate();
  await settle();
  value.connections.update((current) => ({
    ...current,
    sessions: {
      node: { nodeId: "node", generation: 1, failure: null, phase: ConnectionPhase.Disconnected },
    },
  }));
  delayed.resolve(complete());
  await settle();
  expect(value.state.getSnapshot().nodes).toEqual({});
  dispose();
});

test("Tembra reference disposal owns discovery subscription cleanup", async () => {
  const value = await fixture(complete);
  const definition = gitIntegrationOrb(value.presentation);
  await value.host.install(definition);
  const reference = await value.host.get(definition.services["swarmie.git.integration"], "main");
  await settle();
  expect(value.state.getSnapshot().enabled).toBe(true);
  reference.release();
  await settle();
  expect(value.state.getSnapshot()).toEqual({ enabled: false, nodes: {} });
  const acquired = value.acquired();
  value.connections.update((current) => ({ ...current, selected: "another" }));
  await settle();
  expect(value.acquired()).toBe(acquired);
});
