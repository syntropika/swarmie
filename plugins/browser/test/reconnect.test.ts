import { afterEach, expect, test } from "bun:test";

import type { Connector, Node, NodeConnectionState, NodeSession } from "../src/connections/index";
import {
  Connections,
  ConnectionFailureKind,
  ConnectionPhase,
  NodeConnectionError,
  NodeProtocol,
} from "../src/connections/index";

const coordinators: Connections[] = [];
afterEach(() => {
  for (const coordinator of coordinators.splice(0)) {
    coordinator.close();
  }
});
const a: Node = { id: "a", label: "A", url: null, principal: "alice" };
const b: Node = { id: "b", label: "B", url: null, principal: "alice" };
function session(node: Node, closed?: { run: () => void }): NodeSession {
  return {
    identity: { nodeId: node.id, principal: node.principal, protocolVersion: NodeProtocol.Version },
    get: async () => {
      throw new Error("No capability is acquired by this fixture.");
    },
    close: () => closed?.run(),
  };
}
function create(connector: Connector) {
  const states: NodeConnectionState[] = [];
  const connections = new Connections(connector, (state) => {
    states.push(state);
  });
  coordinators.push(connections);
  connections.configure(a);
  connections.configure(b);
  return { connections, states };
}

async function outcome(pending: Promise<unknown>) {
  try {
    await pending;
    return null;
  } catch (error: unknown) {
    return error;
  }
}

test("an intentional disconnect closes a late opening session", async () => {
  const opening = Promise.withResolvers<NodeSession>();
  let closeCount = 0;
  const { connections } = create(async () => await opening.promise);
  const rejected = outcome(connections.open(a.id));
  connections.disconnect(a.id);
  expect(await rejected).toBeInstanceOf(Error);
  opening.resolve(
    session(a, {
      run: () => {
        closeCount += 1;
      },
    }),
  );
  await Bun.sleep(0);
  expect(closeCount).toBe(1);
  expect(connections.state(a.id).phase).toBe(ConnectionPhase.Disconnected);
});

test("nodes have independent lazy sessions and disconnecting A keeps B usable", async () => {
  const opened: string[] = [];
  const closed: string[] = [];
  const { connections } = create(async (node) => {
    opened.push(node.id);
    return session(node, {
      run: () => {
        closed.push(node.id);
      },
    });
  });
  const [left, same] = await Promise.all([connections.open(a.id), connections.open(a.id)]);
  expect(left).toBe(same);
  expect(opened).toEqual([a.id]);
  const right = await connections.open(b.id);
  connections.disconnect(a.id);
  expect(left.isCurrent()).toBe(false);
  expect(right.isCurrent()).toBe(true);
  expect(closed).toEqual([a.id]);
});

test("replacement attempts get new generations and old loss callbacks cannot close them", async () => {
  const losses: ((error: unknown) => void)[] = [];
  const { connections } = create(async (node, _signal, lost) => {
    losses.push(lost);
    return session(node);
  });
  const old = await connections.open(a.id);
  losses[0]?.(new Error("Lost channel"));
  await Bun.sleep(300);
  const current = await connections.open(a.id);
  expect(current.generation).toBeGreaterThan(old.generation);
  losses[0]?.(new Error("Late old channel"));
  expect(current.isCurrent()).toBe(true);
  expect(connections.state(a.id).generation).toBe(current.generation);
  expect(connections.state(a.id).phase).toBe(ConnectionPhase.Connected);
});

test("a wrong node identity is terminal", async () => {
  let attempts = 0;
  const { connections } = create(async (node) => {
    attempts += 1;
    return {
      ...session(node),
      identity: { nodeId: b.id, principal: node.principal, protocolVersion: NodeProtocol.Version },
    };
  });
  expect(connections.open(a.id)).rejects.toMatchObject({
    kind: ConnectionFailureKind.Identity,
  });
  await Bun.sleep(300);
  expect(attempts).toBe(1);
  expect(connections.state(a.id).phase).toBe(ConnectionPhase.Failed);
});

test("a scope change invalidates the old handle and explicitly notifies projection owners", async () => {
  const { connections, states } = create(async (node) => session(node));
  const old = await connections.open(a.id);
  connections.configure({ ...a, principal: "bob" });
  expect(old.isCurrent()).toBe(false);
  expect(states.at(-1)?.failure?.kind).toBe(ConnectionFailureKind.ScopeChanged);
  const replacement = await connections.open(a.id);
  expect(replacement.principal).toBe("bob");
});

test("protocol failures do not leave a dead connected session or retry forever", async () => {
  let lost: ((error: unknown) => void) | undefined;
  let attempts = 0;
  const { connections } = create(async (node, _signal, ended) => {
    attempts += 1;
    lost = ended;
    return session(node);
  });
  const old = await connections.open(a.id);
  lost?.(new NodeConnectionError(ConnectionFailureKind.Protocol, "Invalid peer frame"));
  expect(old.isCurrent()).toBe(false);
  expect(connections.state(a.id).phase).toBe(ConnectionPhase.Failed);
  await Bun.sleep(300);
  expect(attempts).toBe(1);
});

for (const mismatch of [
  {
    identity: { nodeId: a.id, principal: "bob", protocolVersion: NodeProtocol.Version },
    kind: ConnectionFailureKind.Principal,
  },
  {
    identity: { nodeId: a.id, principal: a.principal, protocolVersion: NodeProtocol.Version + 1 },
    kind: ConnectionFailureKind.Version,
  },
]) {
  test(`a ${mismatch.kind} mismatch is terminal and closes the rejected session`, async () => {
    let closed = 0;
    const { connections } = create(async (node) => ({
      ...session(node, {
        run: () => {
          closed += 1;
        },
      }),
      identity: mismatch.identity,
    }));
    const error = await outcome(connections.open(a.id));
    expect(error).toMatchObject({ kind: mismatch.kind });
    expect(connections.state(a.id).phase).toBe(ConnectionPhase.Failed);
    expect(closed).toBe(1);
  });
}
