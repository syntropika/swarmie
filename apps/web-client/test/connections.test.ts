import { afterEach, expect, test } from "bun:test";

import {
  ConnectionFailureKind,
  ConnectionPhase,
  NodeConnectionError,
} from "@swarmie/plugin-client-node-browser";
import type { Connector } from "@swarmie/plugin-client-node-browser";
import type { ConversationExecutionApi } from "@swarmie/plugin-conversations-browser";
import { conversationsOrb, createConversationRecords } from "@swarmie/plugin-conversations-browser";
import { createHost } from "@syntropika/tembra-sdk/node";

import { initializeClient } from "../src/client/initialize";
import type { ClientUi } from "../src/client/initialize";
import { createSubmissions } from "../src/client/submissions";
import { fixtureConnector, fixtureNode } from "./connection-fixture";

const a = { ...fixtureNode, id: "a", label: "A" };
const b = { ...fixtureNode, id: "b", label: "B" };
let client: ClientUi | undefined;
afterEach(async () => {
  await client?.close();
  client = undefined;
});

async function prepare(value: ClientUi) {
  const chat = value.activeConversation();
  value.providers.state.update((state) => ({
    ...state,
    accounts: [
      {
        id: "account",
        providerId: "provider",
        providerLabel: "Provider",
        label: "Account",
        available: true,
        models: [{ id: "model", label: "Model", efforts: [] }],
      },
    ],
  }));
  chat.draft.update((draft) => ({
    ...draft,
    text: "First message",
    account: "account",
    model: "model",
  }));
  await Bun.sleep(10);
  return chat;
}

test("a draft targets B while A is selected and admission freezes that destination", async () => {
  const admission = Promise.withResolvers<{ accepted: true }>();
  const calls: string[] = [];
  const stops: string[] = [];
  client = await initializeClient(createHost(), {
    dev: false,
    nodes: [a, b],
    connector: fixtureConnector,
    execution: (session) => ({
      submit: async (input) => {
        calls.push(`${session.nodeId}:${input.draft.node}`);
        return await admission.promise;
      },
      stop: async () => {
        stops.push(session.nodeId);
        await Promise.resolve();
      },
    }),
  });
  const chat = await prepare(client);
  client.setConversationNode(chat, b.id);
  await Bun.sleep(5);
  client.send(chat);
  client.setConversationNode(chat, a.id);
  client.selectNode(a.id);
  await Bun.sleep(5);
  expect(calls).toEqual(["b:b"]);
  expect(chat.started).toBe(false);
  admission.resolve({ accepted: true });
  await Bun.sleep(10);
  expect(chat.node).toBe(b.id);
  client.setConversationNode(chat, a.id);
  await Bun.sleep(5);
  expect(chat.draft.getSnapshot().node).toBe(b.id);
  expect(client.connectionState.getSnapshot().selected).toBe(a.id);
  client.stop(chat);
  await Bun.sleep(5);
  expect(stops).toEqual([b.id]);
  expect(client.nodeConnections.state(a.id).phase).toBe(ConnectionPhase.Connected);
});

test("an unavailable B preserves the draft and never dispatches to A", async () => {
  const calls: string[] = [];
  const connector: Connector = async (node, signal, lost) => {
    if (node.id === b.id) {
      throw new NodeConnectionError(
        ConnectionFailureKind.Configuration,
        "B is unavailable in this fixture.",
      );
    }
    return await fixtureConnector(node, signal, lost);
  };
  client = await initializeClient(createHost(), {
    dev: false,
    nodes: [a, b],
    connector,
    execution: (session) => ({
      submit: async () => {
        calls.push(session.nodeId);
        await Promise.resolve();
        return { accepted: true };
      },
      stop: async () => {
        await Promise.resolve();
      },
    }),
  });
  const chat = await prepare(client);
  client.setConversationNode(chat, b.id);
  await Bun.sleep(5);
  client.send(chat);
  await Bun.sleep(10);
  expect(calls).toEqual([]);
  expect(chat.draft.getSnapshot().text).toBe("First message");
  expect(chat.draft.getSnapshot().node).toBe(b.id);
  expect(chat.started).toBe(false);
  expect(chat.execution.getSnapshot().phase).toBe("idle");
  expect(client.nodeConnections.state(a.id).phase).toBe(ConnectionPhase.Connected);
});

test("a stop acknowledgement from an old principal cannot replace the current projection", async () => {
  const stopped = Promise.withResolvers<undefined>();
  let dispatched = false;
  client = await initializeClient(createHost(), {
    dev: false,
    nodes: [a],
    connector: fixtureConnector,
    execution: () => ({
      submit: async () => {
        await Promise.resolve();
        return { accepted: true };
      },
      stop: async () => {
        dispatched = true;
        await stopped.promise;
      },
    }),
  });
  const chat = await prepare(client);
  client.send(chat);
  await Bun.sleep(10);
  expect(chat.started).toBe(true);
  client.stop(chat);
  await Bun.sleep(5);
  expect(dispatched).toBe(true);
  client.nodeConnections.configure({ ...a, principal: "another-user" });
  await Bun.sleep(5);
  expect(chat.execution.getSnapshot().phase).toBe("unknown");
  stopped.resolve(undefined);
  await Bun.sleep(5);
  expect(chat.execution.getSnapshot().phase).toBe("unknown");
});

test("unknown first admission stays bound to B and stopping does not enable replay", async () => {
  const submissions: string[] = [];
  const stops: string[] = [];
  client = await initializeClient(createHost(), {
    dev: false,
    nodes: [a, b],
    connector: fixtureConnector,
    execution: (session) => ({
      submit: async () => {
        submissions.push(session.nodeId);
        await Promise.resolve();
        throw new Error("The first acknowledgement was lost.");
      },
      stop: async () => {
        stops.push(session.nodeId);
        await Promise.resolve();
      },
    }),
  });
  const chat = await prepare(client);
  client.setConversationNode(chat, b.id);
  await Bun.sleep(5);
  client.send(chat);
  await Bun.sleep(10);
  expect(chat.started).toBe(false);
  expect(chat.execution.getSnapshot().phase).toBe("unknown");
  client.setConversationNode(chat, a.id);
  client.selectNode(a.id);
  await Bun.sleep(5);
  client.stop(chat);
  await Bun.sleep(10);
  expect(stops).toEqual([b.id]);
  client.send(chat);
  await Bun.sleep(10);
  expect(submissions).toEqual([b.id]);
  expect(chat.execution.getSnapshot().phase).toBe("unknown");
  expect(client.canSend(chat)).toBe(false);
  expect(chat.node).toBe(b.id);
  expect(chat.draft.getSnapshot().text).toBe("First message");
});

test("an inflight or rejected stop cannot unlock unknown first admission", async () => {
  const stopping = Promise.withResolvers<undefined>();
  let submissions = 0;
  client = await initializeClient(createHost(), {
    dev: false,
    nodes: [a],
    connector: fixtureConnector,
    execution: () => ({
      submit: async () => {
        submissions += 1;
        await Promise.resolve();
        throw new Error("Lost admission response");
      },
      stop: async () => {
        await stopping.promise;
      },
    }),
  });
  const chat = await prepare(client);
  client.send(chat);
  await Bun.sleep(10);
  expect(chat.started).toBe(false);
  expect(chat.execution.getSnapshot().phase).toBe("unknown");
  client.stop(chat);
  await Bun.sleep(5);
  expect(chat.execution.getSnapshot().phase).toBe("stopping");
  expect(client.canSend(chat)).toBe(false);
  client.send(chat);
  await Bun.sleep(5);
  expect(submissions).toBe(1);
  stopping.reject(new Error("Cancellation acknowledgement was lost"));
  await Bun.sleep(5);
  expect(chat.execution.getSnapshot().phase).toBe("unknown");
  expect(client.canSend(chat)).toBe(false);
  client.send(chat);
  await Bun.sleep(5);
  expect(submissions).toBe(1);
});

test("overlapping stop acknowledgements preserve unknown admission independently of phase", async () => {
  const first = Promise.withResolvers<undefined>();
  const second = Promise.withResolvers<undefined>();
  let stops = 0;
  client = await initializeClient(createHost(), {
    dev: false,
    nodes: [a],
    connector: fixtureConnector,
    execution: () => ({
      submit: async () => {
        await Promise.resolve();
        throw new Error("Lost admission response");
      },
      stop: async () => {
        stops += 1;
        await (stops === 1 ? first.promise : second.promise);
      },
    }),
  });
  const chat = await prepare(client);
  client.send(chat);
  await Bun.sleep(10);
  client.stop(chat);
  await Bun.sleep(5);
  client.stop(chat);
  await Bun.sleep(5);
  expect(stops).toBe(2);
  first.resolve(undefined);
  await Bun.sleep(5);
  second.resolve(undefined);
  await Bun.sleep(5);
  expect(chat.execution.getSnapshot().phase).toBe("unknown");
  expect(client.canSend(chat)).toBe(false);
});

async function beginStarted(execution: ConversationExecutionApi) {
  client = await initializeClient(createHost(), {
    dev: false,
    nodes: [a],
    connector: fixtureConnector,
    execution: () => execution,
  });
  const chat = await prepare(client);
  client.send(chat);
  await Bun.sleep(10);
  expect(chat.started).toBe(true);
  return { current: client, chat };
}

test("stop cannot unlock a follow-up submission with an unknown outcome", async () => {
  let submissions = 0;
  let stops = 0;
  const { current, chat } = await beginStarted({
    submit: async () => {
      submissions += 1;
      if (submissions > 1) {
        throw new Error("The follow-up acknowledgement was lost.");
      }
      return { accepted: true };
    },
    stop: async () => {
      stops += 1;
    },
  });
  chat.draft.update((draft) => ({ ...draft, text: "Follow-up" }));
  await Bun.sleep(5);
  current.send(chat);
  await Bun.sleep(10);
  expect(chat.execution.getSnapshot().phase).toBe("unknown");
  const beforeStop = chat.execution.getSnapshot();
  current.stop(chat);
  await Bun.sleep(10);
  expect(stops).toBe(1);
  expect(chat.execution.getSnapshot()).toEqual(beforeStop);
  expect(current.canSend(chat)).toBe(false);
  expect(chat.draft.getSnapshot().text).toBe("Follow-up");
  current.send(chat);
  await Bun.sleep(5);
  expect(submissions).toBe(2);
});

test("stop does not replace uncertainty caused by a changed principal", async () => {
  let stops = 0;
  const { current, chat } = await beginStarted({
    submit: async () => ({ accepted: true }),
    stop: async () => {
      stops += 1;
    },
  });
  chat.draft.update((draft) => ({ ...draft, text: "Do not replay" }));
  chat.execution.update((value) => ({ ...value, phase: "unknown", canAcknowledge: true }));
  await Bun.sleep(5);
  current.nodeConnections.configure({ ...a, principal: "another-user" });
  await Bun.sleep(5);
  expect(chat.execution.getSnapshot().phase).toBe("unknown");
  expect(chat.execution.getSnapshot().canAcknowledge).toBe(false);
  current.stop(chat);
  await Bun.sleep(10);
  expect(stops).toBe(1);
  expect(chat.execution.getSnapshot().phase).toBe("unknown");
  expect(chat.execution.getSnapshot().label).toBe("Node access changed.");
  expect(current.canSend(chat)).toBe(false);
  expect(chat.draft.getSnapshot().text).toBe("Do not replay");
});

test("a later stop acknowledgement preserves a concurrently unknown follow-up", async () => {
  const following = Promise.withResolvers<{ accepted: true }>();
  const stopping = Promise.withResolvers<undefined>();
  let submissions = 0;
  let stops = 0;
  const { current, chat } = await beginStarted({
    submit: async () => {
      submissions += 1;
      return submissions === 1 ? { accepted: true } : await following.promise;
    },
    stop: async () => {
      stops += 1;
      await stopping.promise;
    },
  });
  chat.draft.update((draft) => ({ ...draft, text: "Follow-up" }));
  await Bun.sleep(5);
  current.send(chat);
  await Bun.sleep(5);
  current.stop(chat);
  await Bun.sleep(5);
  expect(stops).toBe(1);
  following.reject(new Error("The follow-up acknowledgement was lost."));
  await Bun.sleep(5);
  expect(chat.execution.getSnapshot().phase).toBe("unknown");
  stopping.resolve(undefined);
  await Bun.sleep(5);
  expect(chat.execution.getSnapshot().phase).toBe("unknown");
  expect(current.canSend(chat)).toBe(false);
});

test("a failed stop restores the prior active projection without claiming cancellation", async () => {
  const stopping = Promise.withResolvers<undefined>();
  const { current, chat } = await beginStarted({
    submit: async () => ({ accepted: true }),
    stop: async () => {
      await stopping.promise;
    },
  });
  const before = chat.execution.getSnapshot();
  current.stop(chat);
  await Bun.sleep(5);
  expect(chat.execution.getSnapshot().phase).toBe("stopping");
  stopping.reject(new Error("Cancellation failed."));
  await Bun.sleep(5);
  expect(chat.execution.getSnapshot().phase).toBe(before.phase);
  expect(chat.execution.getSnapshot().label).toBe(before.label);
});

test("stop on an unsubmitted draft does not dispatch a cancellation", async () => {
  let stops = 0;
  client = await initializeClient(createHost(), {
    dev: false,
    nodes: [a],
    connector: fixtureConnector,
    execution: () => ({
      submit: async () => ({ accepted: true }),
      stop: async () => {
        stops += 1;
      },
    }),
  });
  const chat = await prepare(client);
  client.stop(chat);
  await Bun.sleep(5);
  expect(stops).toBe(0);
  expect(chat.execution.getSnapshot().phase).toBe("idle");
});

test("stop still dispatches after a follow-up fails before admission", async () => {
  let factories = 0;
  let stops = 0;
  client = await initializeClient(createHost(), {
    dev: false,
    nodes: [a],
    connector: fixtureConnector,
    execution: () => {
      factories += 1;
      if (factories === 2) {
        throw new Error("The follow-up adapter was unavailable.");
      }
      return {
        submit: async () => ({ accepted: true }),
        stop: async () => {
          stops += 1;
        },
      };
    },
  });
  const chat = await prepare(client);
  client.send(chat);
  await Bun.sleep(10);
  expect(chat.started).toBe(true);
  expect(chat.execution.getSnapshot().phase).toBe("working");
  chat.draft.update((draft) => ({ ...draft, text: "Not dispatched" }));
  await Bun.sleep(5);
  client.send(chat);
  await Bun.sleep(5);
  expect(chat.execution.getSnapshot().phase).toBe("idle");
  client.stop(chat);
  await Bun.sleep(5);
  expect(factories).toBe(3);
  expect(stops).toBe(1);
  expect(chat.execution.getSnapshot().phase).toBe("stopped");
  expect(chat.draft.getSnapshot().text).toBe("Not dispatched");
});

test("an old stop acknowledgement does not overwrite a newly accepted follow-up", async () => {
  const stopping = Promise.withResolvers<undefined>();
  let stops = 0;
  const { current, chat } = await beginStarted({
    submit: async () => ({ accepted: true }),
    stop: async () => {
      stops += 1;
      if (stops === 1) {
        await stopping.promise;
      }
    },
  });
  current.stop(chat);
  await Bun.sleep(5);
  expect(chat.execution.getSnapshot().phase).toBe("stopping");
  chat.draft.update((draft) => ({ ...draft, text: "New turn" }));
  await Bun.sleep(5);
  current.send(chat);
  await Bun.sleep(5);
  expect(chat.execution.getSnapshot().phase).toBe("working");
  stopping.resolve(undefined);
  await Bun.sleep(5);
  expect(chat.execution.getSnapshot().phase).toBe("working");
  current.stop(chat);
  await Bun.sleep(5);
  expect(stops).toBe(2);
  expect(chat.execution.getSnapshot().phase).toBe("stopped");
});

for (const outcomes of [
  [false, true],
  [true, false],
  [false, false],
  [true, true],
] as const) {
  for (const order of [
    [0, 1],
    [1, 0],
  ] as const) {
    test(`overlapping stops ${outcomes.join("/")} settled ${order.join("/")} preserve their shared outcome`, async () => {
      const attempts = [Promise.withResolvers<undefined>(), Promise.withResolvers<undefined>()];
      let calls = 0;
      const { current, chat } = await beginStarted({
        submit: async () => ({ accepted: true }),
        stop: async () => {
          const attempt = attempts[calls];
          calls += 1;
          if (!attempt) {
            throw new Error("Unexpected stop attempt.");
          }
          await attempt.promise;
        },
      });
      const previous = chat.execution.getSnapshot();
      current.stop(chat);
      current.stop(chat);
      await Bun.sleep(5);
      expect(calls).toBe(2);
      expect(chat.execution.getSnapshot().phase).toBe("stopping");
      function complete(index: 0 | 1) {
        const attempt = attempts[index];
        if (!attempt) {
          throw new Error("Missing stop attempt.");
        }
        if (outcomes[index]) {
          attempt.resolve(undefined);
        } else {
          attempt.reject(new Error("Cancellation failed."));
        }
      }
      complete(order[0]);
      await Bun.sleep(5);
      expect(chat.execution.getSnapshot().phase).toBe(outcomes[order[0]] ? "stopped" : "stopping");
      complete(order[1]);
      await Bun.sleep(5);
      expect(chat.execution.getSnapshot().phase).toBe(
        outcomes.some(Boolean) ? "stopped" : previous.phase,
      );
      if (!outcomes.some(Boolean)) {
        expect(chat.execution.getSnapshot().label).toBe(previous.label);
      }
    });
  }
}

test("an earlier stop batch cannot finish a newer stop after an accepted turn", async () => {
  const attempts = [
    Promise.withResolvers<undefined>(),
    Promise.withResolvers<undefined>(),
    Promise.withResolvers<undefined>(),
  ];
  let calls = 0;
  const { current, chat } = await beginStarted({
    submit: async () => ({ accepted: true }),
    stop: async () => {
      const attempt = attempts[calls];
      calls += 1;
      if (!attempt) {
        throw new Error("Unexpected stop attempt.");
      }
      await attempt.promise;
    },
  });
  current.stop(chat);
  current.stop(chat);
  await Bun.sleep(5);
  chat.draft.update((draft) => ({ ...draft, text: "New turn" }));
  await Bun.sleep(5);
  current.send(chat);
  await Bun.sleep(5);
  expect(chat.execution.getSnapshot().phase).toBe("working");
  current.stop(chat);
  await Bun.sleep(5);
  expect(calls).toBe(3);
  attempts[0]?.resolve(undefined);
  await Bun.sleep(5);
  expect(chat.execution.getSnapshot().phase).toBe("stopping");
  attempts[1]?.reject(new Error("Old cancellation failed."));
  await Bun.sleep(5);
  expect(chat.execution.getSnapshot().phase).toBe("stopping");
  attempts[2]?.reject(new Error("New cancellation failed."));
  await Bun.sleep(5);
  expect(chat.execution.getSnapshot().phase).toBe("working");
});

for (const secondSucceeds of [true, false]) {
  test(`same-tick stop settlements preserve ${secondSucceeds ? "success" : "failure"}`, async () => {
    const attempts = [Promise.withResolvers<undefined>(), Promise.withResolvers<undefined>()];
    let calls = 0;
    const { current, chat } = await beginStarted({
      submit: async () => ({ accepted: true }),
      stop: async () => {
        const attempt = attempts[calls];
        calls += 1;
        if (!attempt) {
          throw new Error("Unexpected stop attempt.");
        }
        await attempt.promise;
      },
    });
    current.stop(chat);
    current.stop(chat);
    await Bun.sleep(5);
    expect(calls).toBe(2);
    attempts[0]?.reject(new Error("First cancellation failed."));
    if (secondSucceeds) {
      attempts[1]?.resolve(undefined);
    } else {
      attempts[1]?.reject(new Error("Second cancellation failed."));
    }
    await Bun.sleep(10);
    expect(chat.execution.getSnapshot().phase).toBe(secondSucceeds ? "stopped" : "working");
  });

  test(`deferred stop commits preserve ${secondSucceeds ? "success" : "failure"} after an earlier commit returns`, async () => {
    await checkDeferredStopCommits(secondSucceeds);
  });
}

async function checkDeferredStopCommits(secondSucceeds: boolean) {
  const host = createHost();
  const attempts = [Promise.withResolvers<undefined>(), Promise.withResolvers<undefined>()];
  const firstApplied = Promise.withResolvers<undefined>();
  const firstReturn = Promise.withResolvers<undefined>();
  const secondWaiting = Promise.withResolvers<undefined>();
  const secondApply = Promise.withResolvers<undefined>();
  const secondApplied = Promise.withResolvers<undefined>();
  let calls = 0;
  const execution: ConversationExecutionApi = {
    submit: async () => ({ accepted: true }),
    stop: async () => {
      const attempt = attempts[calls];
      calls += 1;
      if (!attempt) {
        throw new Error("Unexpected stop attempt.");
      }
      await attempt.promise;
    },
  };
  client = await initializeClient(host, {
    dev: false,
    nodes: [a],
    connector: fixtureConnector,
    execution: () => execution,
  });
  const owner = await host.get(conversationsOrb.services["swarmie.conversations.ui"], "main");
  try {
    const chat = await prepare(client);
    client.send(chat);
    await Bun.sleep(10);
    expect(chat.started).toBe(true);
    let commits = 0;
    const records = createConversationRecords(client.conversationState, async (change) => {
      commits += 1;
      if (commits === 1) {
        await owner.update(change);
        firstApplied.resolve(undefined);
        await firstReturn.promise;
        return;
      }
      secondWaiting.resolve(undefined);
      await secondApply.promise;
      await owner.update(change);
      secondApplied.resolve(undefined);
    });
    const submissions = createSubmissions(
      client.nodeConnections,
      client.connectionState,
      records,
      client.providers,
      () => execution,
      client.conversationsUi.lifecycle,
      client.nextId,
    );
    submissions.stop(chat);
    submissions.stop(chat);
    await Bun.sleep(5);
    expect(calls).toBe(2);
    attempts[0]?.reject(new Error("First cancellation failed."));
    await firstApplied.promise;
    expect(chat.execution.getSnapshot().phase).toBe("stopping");
    if (secondSucceeds) {
      attempts[1]?.resolve(undefined);
    } else {
      attempts[1]?.reject(new Error("Second cancellation failed."));
    }
    await secondWaiting.promise;
    firstReturn.resolve(undefined);
    await Bun.sleep(5);
    secondApply.resolve(undefined);
    await secondApplied.promise;
    await Bun.sleep(5);
    expect(chat.execution.getSnapshot().phase).toBe(secondSucceeds ? "stopped" : "working");
  } finally {
    attempts[0]?.resolve(undefined);
    attempts[1]?.resolve(undefined);
    firstReturn.resolve(undefined);
    secondApply.resolve(undefined);
    owner.release();
  }
}
