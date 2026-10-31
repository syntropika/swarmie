import { afterEach, expect, test } from "bun:test";

import { observeNodeService, NodeProtocol } from "@swarmie/plugin-browser/connections";
import type { Node } from "@swarmie/plugin-browser/connections";
import type { ProjectsCommand } from "@swarmie/plugin-projects-browser";
import { orb } from "@syntropika/tembra-sdk";
import type { State, Context } from "@syntropika/tembra-sdk";
import { createHost } from "@syntropika/tembra-sdk/node";

import { fixtureProjects } from "../../../tests/fixtures/client-ui/projects";
import { initializeClient } from "../src/client/initialize";

const closes: (() => Promise<void>)[] = [];
afterEach(async () => {
  await Promise.all(
    closes
      .splice(0)
      .toReversed()
      .map(async (close) => {
        await close();
      }),
  );
});
async function until(predicate: () => boolean, remaining = 100): Promise<void> {
  if (predicate()) {
    return;
  }
  if (remaining === 0) {
    throw new Error("The collection projection did not settle.");
  }
  await Bun.sleep(5);
  await until(predicate, remaining - 1);
}
async function fixture() {
  const backend = createHost();
  closes.push(async () => {
    await backend.close();
  });
  const data = fixtureProjects();
  let wait: Promise<void> = Promise.resolve();
  let loseReply = false;
  let writes = 0;
  let suppressPublication = false;
  let nextRead: { entered: () => void; wait: Promise<undefined> } | null = null;
  const definition = orb({
    name: "fixture.collections",
    apiVersion: 1,
    services: {
      "swarmie.projects": {
        create: (self) => self.state(data.snapshot()),
        methods: {
          async list() {
            const snapshot = data.snapshot();
            const gate = nextRead;
            nextRead = null;
            if (gate !== null) {
              gate.entered();
              await gate.wait;
            }
            return { revision: snapshot.revision, data: JSON.stringify(snapshot), next: null };
          },
          async change(
            state: State<ReturnType<typeof data.snapshot>>,
            context: Context,
            command: ProjectsCommand,
          ) {
            writes += 1;
            await data.change(command);
            const next = data.snapshot();
            if (!suppressPublication) {
              state.update(context, () => next);
            }
            await wait;
            if (loseReply) {
              loseReply = false;
              throw new Error("Synthetic reply loss");
            }
            return { revision: next.revision };
          },
        },
      },
      "swarmie.activity": {
        create: (self) =>
          self.state({
            revision: 1,
            items: [
              {
                id: JSON.stringify(["tasks", "one"]),
                owner: "tasks",
                entity: "one",
                title: "Complete",
                text: "Ready",
                unread: true,
                updatedAt: 1,
              },
            ],
          }),
        methods: {
          list: (
            state: State<{
              revision: number;
              items: {
                id: string;
                owner: string;
                entity: string;
                title: string;
                text: string;
                unread: boolean;
                updatedAt: number;
              }[];
            }>,
          ) => ({ revision: state.value.revision, data: JSON.stringify(state.value), next: null }),
          change(
            state: State<{
              revision: number;
              items: {
                id: string;
                owner: string;
                entity: string;
                title: string;
                text: string;
                unread: boolean;
                updatedAt: number;
              }[];
            }>,
            context: Context,
            command: { kind: string; id: string },
          ) {
            if (command.kind !== "read") {
              throw new Error("Fixture supports marking read only.");
            }
            state.update(context, (current) => ({
              revision: current.revision + 1,
              items: current.items.map((item) =>
                item.id === command.id ? { ...item, unread: false } : item,
              ),
            }));
            return { revision: state.value.revision };
          },
        },
      },
    },
  });
  await backend.install(definition);
  const node: Node = { id: "fixture", label: "Fixture", url: null, principal: "alice" };
  const client = await initializeClient(createHost(), {
    dev: false,
    nodes: [node],
    connector: async (selected) => ({
      identity: {
        nodeId: selected.id,
        principal: selected.principal,
        protocolVersion: NodeProtocol.Version,
      },
      get: async (service, key) => await backend.get(service, key),
      close: () => undefined,
    }),
  });
  closes.push(client.close);
  await until(
    () =>
      client.projectsState.getSnapshot().available &&
      client.activityApi.state.getSnapshot().available,
  );
  return {
    holdNextRead: () => {
      const entered = Promise.withResolvers<undefined>();
      const released = Promise.withResolvers<undefined>();
      nextRead = {
        entered: () => {
          entered.resolve(undefined);
        },
        wait: released.promise,
      };
      return {
        entered: entered.promise,
        release: () => {
          released.resolve(undefined);
        },
      };
    },
    suppressPublication: () => {
      suppressPublication = true;
    },
    client,
    data,
    gate: (pending: Promise<void>) => {
      wait = pending;
    },
    loseNext: () => {
      loseReply = true;
    },
    writes: () => writes,
  };
}

test("Projects uses the native service and Activity observes and marks the native inbox", async () => {
  const { client, data } = await fixture();
  const project = await client.projects.create({ name: "Native project" });
  expect(data.snapshot().records[0]?.id).toBe(project.id);
  expect(client.projectsState.getSnapshot().nodeId).toBe("fixture");
  const [item] = client.activity.getSnapshot();
  expect(item?.unread).toBe(true);
  if (!item) {
    throw new Error("Missing fixture activity entry");
  }
  await client.activityApi.read(item.id);
  await until(() => client.activity.getSnapshot()[0]?.unread === false);
});

test("an uncertain project mutation reconciles by reading and is never replayed", async () => {
  const { client, data, loseNext, writes } = await fixture();
  loseNext();
  let failure: unknown;
  try {
    await client.projects.create({ name: "Confirmed despite lost reply" });
  } catch (error: unknown) {
    failure = error;
  }
  expect(failure).toBeInstanceOf(Error);
  expect(data.snapshot().records).toHaveLength(1);
  expect(writes()).toBe(1);
  await until(() => client.projectsState.getSnapshot().records.length === 1);
});

test("disconnect fences pending project projection updates and retains same-scope unadmitted draft links", async () => {
  const { client, data, gate } = await fixture();
  const project = await client.projects.create({ name: "First" });
  await client.projects.stageConversation("draft", project.id);
  const pending = Promise.withResolvers<undefined>();
  gate(pending.promise);
  const request = (async () => {
    try {
      await client.projects.create({ name: "Pending" });
      return null;
    } catch (error: unknown) {
      return error;
    }
  })();
  await until(() => data.snapshot().records.length === 2);
  client.disconnect();
  await until(() => !client.projectsState.getSnapshot().available);
  pending.resolve(undefined);
  expect(await request).toBe(null);
  expect(client.projectsState.getSnapshot().draftLinks.draft).toBe(project.id);
  expect(client.projectsState.getSnapshot().nodeId).toBe("fixture");
});

test("a confirmed receipt supersedes an older read even when State publication fails", async () => {
  const setup = await fixture();
  const opened = Promise.withResolvers<undefined>();
  const failures: unknown[] = [];
  const binding = observeNodeService(
    setup.client.connectionState,
    setup.client.nodeConnections,
    "swarmie.projects",
    {
      reset: () => {
        /* The test observes revision publication separately. */
      },
      receive: () => {
        /* The complete returned snapshot is asserted below. */
      },
      failed: (error) => {
        failures.push(error);
      },
      ready: () => {
        opened.resolve(undefined);
      },
    },
  );
  await opened.promise;
  await Bun.sleep(5);
  const gate = setup.holdNextRead();
  const older = binding.read();
  await gate.entered;
  setup.suppressPublication();
  const receipt = await binding.invoke("change", {
    kind: "create",
    id: "committed",
    name: "Committed",
    description: "",
  });
  const after = binding.read();
  gate.release();
  const [previous, current] = await Promise.all([older, after]);
  expect(receipt).toMatchObject({ revision: 1 });
  expect(previous).toMatchObject({ revision: 1 });
  expect(current).toMatchObject({ revision: 1, records: [{ id: "committed" }] });
  expect(setup.writes()).toBe(1);
  expect(failures).toEqual([]);
  binding.close();
});

test.each(["switch", "reconnect"] as const)(
  "old Activity receipts cannot read or retire the replacement generation during %s",
  async (mode) => {
    const releaseOld = Promise.withResolvers<undefined>();
    const oldEntered = Promise.withResolvers<undefined>();
    const releaseNew = Promise.withResolvers<undefined>();
    const newEntered = Promise.withResolvers<undefined>();
    const nodes: Node[] = ["a", "b"].map((id) => ({
      id,
      label: id,
      url: null,
      principal: "alice",
    }));
    let newReads = 0;
    const hosts = await Promise.all(
      nodes.map(async (node, index) => {
        const host = createHost();
        closes.push(async () => {
          releaseOld.resolve(undefined);
          releaseNew.resolve(undefined);
          await host.close();
        });
        await host.install(
          orb({
            name: "fixture.activity.generation",
            apiVersion: 1,
            services: {
              "swarmie.activity": {
                create: (self) => self.state({ revision: 0 }),
                methods: {
                  async list(state: State<{ revision: number }>) {
                    if (index === 1) {
                      newReads += 1;
                      newEntered.resolve(undefined);
                      await releaseNew.promise;
                    }
                    const value = { revision: state.value.revision, items: [] };
                    return { revision: value.revision, data: JSON.stringify(value), next: null };
                  },
                  async change(state: State<{ revision: number }>, context: Context) {
                    state.update(context, (current) => ({ revision: current.revision + 1 }));
                    if (index === 0) {
                      oldEntered.resolve(undefined);
                      await releaseOld.promise;
                    }
                    return { revision: state.value.revision };
                  },
                },
              },
            },
          }),
        );
        return { node, host };
      }),
    );
    let replacement = false;
    const client = await initializeClient(createHost(), {
      dev: false,
      nodes,
      connector: async (node) => {
        const server =
          mode === "reconnect" && replacement
            ? hosts[1]
            : hosts.find((entry) => entry.node.id === node.id);
        if (server === undefined) {
          throw new Error("Fixture host is unavailable.");
        }
        return {
          identity: {
            nodeId: node.id,
            principal: node.principal,
            protocolVersion: NodeProtocol.Version,
          },
          get: async (service, key) => await server.host.get(service, key),
          close: () => undefined,
        };
      },
    });
    closes.push(client.close);
    await until(() => client.activityApi.state.getSnapshot().available);
    const settled = Promise.withResolvers<undefined>();
    const publishing = (async () => {
      await client.activityApi.publish({
        owner: "fixture",
        entity: "one",
        title: "Complete",
        text: "Ready",
      });
      settled.resolve(undefined);
    })();
    await oldEntered.promise;
    if (mode === "switch") {
      client.selectNode("b");
    } else {
      client.disconnect("a");
      replacement = true;
    }
    await client.connect(mode === "switch" ? "b" : "a");
    await newEntered.promise;
    releaseOld.resolve(undefined);
    await settled.promise;
    expect(newReads).toBe(1);
    releaseNew.resolve(undefined);
    await until(() => client.activityApi.state.getSnapshot().available);
    expect(client.activityApi.state.getSnapshot().error).toBe(null);
    expect(client.activityApi.state.getSnapshot().revision).toBe(0);
    await publishing;
  },
);
