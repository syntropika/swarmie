import { afterEach, expect, test } from "bun:test";

import type { DynamicReference, State } from "@syntropika/tembra-sdk";
import { Code, orb } from "@syntropika/tembra-sdk";
import { createHost } from "@syntropika/tembra-sdk/node";
import { serve } from "@syntropika/tembra-sdk/rpc";
import type { ServerWebSocket } from "bun";

import {
  Connections,
  ConnectionFailureKind,
  ConnectionPhase,
  NodeProtocol,
  connectWebSocket,
} from "../src/index";

const cleanup: (() => void | Promise<void>)[] = [];
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

async function node(id: string, wait?: Promise<undefined>) {
  const host = createHost();
  let pending = 0;
  await host.install(
    orb({
      name: `fixture.${id}`,
      apiVersion: 1,
      services: {
        [NodeProtocol.Service]: {
          create: () => undefined,
          methods: {
            identify(_state, context) {
              return {
                nodeId: id,
                principal: context.principal,
                protocolVersion: NodeProtocol.Version,
              };
            },
          },
        },
        "fixture.counter": {
          create: (self) => self.state(0),
          methods: {
            increment(state: State<number>, context) {
              state.update(context, (value) => value + 1);
              return state.value;
            },
            async hold(state: State<number>) {
              pending += 1;
              await wait;
              return state.value;
            },
          },
        },
      },
    }),
  );
  type Peer = {
    receive: (bytes: Uint8Array) => void;
    closed: (error?: unknown) => void;
    close: () => void;
  };
  const peers = new Map<ServerWebSocket, Peer>();
  const server = Bun.serve({
    hostname: "127.0.0.1",
    port: 0,
    fetch(request, owner) {
      if (owner.upgrade(request)) {
        return undefined;
      }
      return new Response("WebSocket required", { status: 400 });
    },
    websocket: {
      open(socket) {
        const peer: Peer = {
          receive: () => undefined,
          closed: () => undefined,
          close: () => undefined,
        };
        peers.set(socket, peer);
        const session = serve(
          host,
          {
            send(frame) {
              socket.send(new Uint8Array(frame));
            },
            listen(receive, closed) {
              peer.receive = receive;
              peer.closed = closed;
              return () => {
                peer.receive = () => undefined;
              };
            },
            close() {
              socket.close();
            },
          },
          { principal: "alice" },
        );
        peer.close = () => {
          session.close();
        };
      },
      message(socket, bytes) {
        if (typeof bytes !== "string") {
          peers.get(socket)?.receive(new Uint8Array(bytes));
        }
      },
      close(socket) {
        peers.get(socket)?.closed();
        peers.get(socket)?.close();
        peers.delete(socket);
      },
    },
  });
  cleanup.push(async () => {
    for (const peer of peers.values()) {
      peer.close();
    }
    await server.stop(true);
    await host.close();
  });
  return {
    record: { id, label: id, url: `ws://127.0.0.1:${server.port}`, principal: "alice" },
    peers,
    pending: () => pending,
  };
}

async function until(predicate: () => boolean, remaining = 100): Promise<void> {
  if (predicate()) {
    return;
  }
  if (remaining === 0) {
    throw new Error("Connection state did not settle.");
  }
  await Bun.sleep(5);
  await until(predicate, remaining - 1);
}

function counterValue(reference: DynamicReference) {
  const value: unknown = reference.state.value;
  if (typeof value !== "number") {
    throw new TypeError("The fixture counter must be numeric.");
  }
  return value;
}

async function increment(reference: DynamicReference) {
  const method: unknown = Reflect.get(reference, "increment");
  if (typeof method !== "function") {
    throw new TypeError("Counter method is unavailable.");
  }
  await Reflect.apply(method, undefined, []);
}

test("two real SDK WebSocket endpoints keep equal resource keys isolated", async () => {
  const [a, b] = await Promise.all([node("a"), node("b")]);
  const connections = new Connections(connectWebSocket, () => undefined);
  cleanup.push(() => {
    connections.close();
  });
  connections.configure(a.record);
  connections.configure(b.record);
  const [left, right] = await Promise.all([
    connections.open(a.record.id),
    connections.open(b.record.id),
  ]);
  const ra = await left.get("fixture.counter", "same");
  const rb = await right.get("fixture.counter", "same");
  await increment(rb);
  expect(counterValue(ra)).toBe(0);
  expect(counterValue(rb)).toBe(1);
  connections.disconnect(a.record.id);
  expect(right.isCurrent()).toBe(true);
  await increment(rb);
  expect(counterValue(rb)).toBe(2);
  ra.release();
  rb.release();
});

test("SDK-triggered protocol closure reports its cause using the published SDK", async () => {
  const endpoint = await node("a");
  const connections = new Connections(connectWebSocket, () => undefined);
  cleanup.push(() => {
    connections.close();
  });
  connections.configure(endpoint.record);
  const session = await connections.open(endpoint.record.id);
  for (const socket of endpoint.peers.keys()) {
    socket.send(new TextEncoder().encode("[]"));
  }
  await until(() => connections.state(endpoint.record.id).phase === ConnectionPhase.Failed);
  expect(connections.state(endpoint.record.id).failure?.kind).toBe(ConnectionFailureKind.Protocol);
  expect(session.isCurrent()).toBe(false);
});

test("a socket from another node is refused before readiness", async () => {
  const endpoint = await node("actual");
  const connections = new Connections(connectWebSocket, () => undefined);
  cleanup.push(() => {
    connections.close();
  });
  connections.configure({ ...endpoint.record, id: "expected" });
  expect(connections.open("expected")).rejects.toMatchObject({
    kind: ConnectionFailureKind.Identity,
  });
  expect(connections.state("expected").phase).toBe(ConnectionPhase.Failed);
});

test("an observer failure does not close the channel or poison a later closure", async () => {
  const endpoint = await node("a");
  const connections = new Connections(connectWebSocket, () => undefined);
  cleanup.push(() => {
    connections.close();
  });
  connections.configure(endpoint.record);
  const session = await connections.open(endpoint.record.id);
  const counter = await session.get("fixture.counter", "main");
  let observed = 0;
  const detach = counter.state.subscribe((value: unknown) => {
    if (typeof value === "number" && value > 0) {
      observed += 1;
      throw new Error("A presentation observer failed.");
    }
  });
  await increment(counter);
  await until(() => observed > 0);
  expect(session.isCurrent()).toBe(true);
  await increment(counter);
  expect(counterValue(counter)).toBe(2);
  detach();
  counter.release();
  for (const socket of endpoint.peers.keys()) {
    socket.send(new TextEncoder().encode("[]"));
  }
  await until(() => connections.state(endpoint.record.id).phase === ConnectionPhase.Failed);
  expect(connections.state(endpoint.record.id).failure?.kind).toBe(ConnectionFailureKind.Protocol);
});

test("an incomplete State event is a terminal protocol failure", async () => {
  const endpoint = await node("a");
  const connections = new Connections(connectWebSocket, () => undefined);
  cleanup.push(() => {
    connections.close();
  });
  connections.configure(endpoint.record);
  const session = await connections.open(endpoint.record.id);
  for (const socket of endpoint.peers.keys()) {
    socket.send(new TextEncoder().encode(JSON.stringify({ event: "state" })));
  }
  await until(() => !session.isCurrent());
  expect(connections.state(endpoint.record.id).phase).toBe(ConnectionPhase.Failed);
  expect(connections.state(endpoint.record.id).failure?.kind).toBe(ConnectionFailureKind.Protocol);
});

test("a malformed URL is a terminal configuration failure", async () => {
  const connections = new Connections(connectWebSocket, () => undefined);
  cleanup.push(() => {
    connections.close();
  });
  connections.configure({ id: "a", label: "A", principal: "alice", url: "not a URL" });
  expect(connections.open("a")).rejects.toMatchObject({
    kind: ConnectionFailureKind.Configuration,
  });
  expect(connections.state("a").phase).toBe(ConnectionPhase.Failed);
});

test("an oversized outgoing call is refused locally without ending the session", async () => {
  const endpoint = await node("a");
  const connections = new Connections(connectWebSocket, () => undefined);
  cleanup.push(() => {
    connections.close();
  });
  connections.configure(endpoint.record);
  const session = await connections.open(endpoint.record.id);
  const counter = await session.get("fixture.counter", "main");
  const method: unknown = Reflect.get(counter, "increment");
  if (typeof method !== "function") {
    throw new TypeError("Missing increment method.");
  }
  expect(
    Reflect.apply(method, undefined, ["x".repeat(NodeProtocol.MaxFrame)]),
  ).rejects.toMatchObject({ code: Code.Exhausted });
  expect(session.isCurrent()).toBe(true);
  expect(counterValue(counter)).toBe(0);
  await increment(counter);
  expect(counterValue(counter)).toBe(1);
  counter.release();
});

test("a peer policy close is terminal instead of an endless reconnect", async () => {
  const endpoint = await node("a");
  const connections = new Connections(connectWebSocket, () => undefined);
  cleanup.push(() => {
    connections.close();
  });
  connections.configure(endpoint.record);
  const session = await connections.open(endpoint.record.id);
  for (const socket of endpoint.peers.keys()) {
    socket.close(1008, "Policy refused");
  }
  await until(() => !session.isCurrent());
  expect(connections.state(endpoint.record.id).phase).toBe(ConnectionPhase.Failed);
  expect(connections.state(endpoint.record.id).failure?.kind).toBe(ConnectionFailureKind.Protocol);
});

test("a seventeenth pending call is refused locally and the WebSocket remains usable", async () => {
  const finish = Promise.withResolvers<undefined>();
  cleanup.push(() => {
    finish.resolve(undefined);
  });
  const endpoint = await node("bounded", finish.promise);
  const connections = new Connections(connectWebSocket, () => undefined);
  cleanup.push(() => {
    connections.close();
  });
  connections.configure(endpoint.record);
  const session = await connections.open(endpoint.record.id);
  const counter = await session.get("fixture.counter", "main");
  const hold: unknown = Reflect.get(counter, "hold");
  if (typeof hold !== "function") {
    throw new TypeError("Missing hold method.");
  }
  const operations = Array.from({ length: NodeProtocol.MaxPending }, async () => {
    await Reflect.apply(hold, undefined, []);
  });
  let rejected: unknown;
  try {
    await Reflect.apply(hold, undefined, []);
  } catch (error: unknown) {
    rejected = error;
  }
  expect(rejected).toMatchObject({ code: Code.Exhausted });
  await until(() => endpoint.pending() === NodeProtocol.MaxPending);
  expect(endpoint.pending()).toBe(16);
  expect(session.isCurrent()).toBe(true);
  finish.resolve(undefined);
  await Promise.all(operations);
  await increment(counter);
  expect(counterValue(counter)).toBe(1);
  expect(session.isCurrent()).toBe(true);
  counter.release();
});
