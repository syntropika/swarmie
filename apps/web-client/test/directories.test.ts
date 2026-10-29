import { afterEach, expect, test } from "bun:test";

import { NodeProtocol } from "@swarmie/plugin-connections-browser";
import { orb } from "@syntropika/tembra-sdk";
import { createHost } from "@syntropika/tembra-sdk/node";

import { directoryPage } from "../src/client/directories";
import { initializeClient } from "../src/client/initialize";

const cleanup: (() => Promise<void>)[] = [];
async function closeAll(): Promise<void> {
  const close = cleanup.pop();
  if (close) {
    await close();
    await closeAll();
  }
}
afterEach(closeAll);

async function outcome(operation: Promise<unknown>) {
  try {
    await operation;
    return null;
  } catch (error: unknown) {
    return error;
  }
}

const root = {
  path: "/allowed",
  parent: null,
  directories: [{ name: "project", path: "/allowed/project" }],
  next: null,
};
async function setup(
  read: (input: { path: string | null; cursor: string | null }) => Promise<unknown>,
) {
  const backend = createHost();
  await backend.install(
    orb({
      name: "fixture.directories",
      apiVersion: 1,
      services: {
        "swarmie.directories": {
          create: () => undefined,
          methods: {
            list: async (_value, _context, input: { path: string | null; cursor: string | null }) =>
              await read(input),
          },
        },
      },
    }),
  );
  cleanup.push(async () => {
    await backend.close();
  });
  const acquired: string[] = [];
  const client = await initializeClient(createHost(), {
    dev: false,
    nodes: [{ id: "node", label: "Local", principal: "user", url: null }],
    connector: async (node) => ({
      identity: {
        nodeId: node.id,
        principal: node.principal,
        protocolVersion: NodeProtocol.Version,
      },
      get: async (service, key) => {
        acquired.push(service);
        return await backend.get(service, key);
      },
      close: () => undefined,
    }),
  });
  cleanup.push(client.close);
  return { client, acquired };
}

test("folder browsing uses the node service and preserves returned paths and page cursors", async () => {
  const calls: unknown[] = [];
  const { client, acquired } = await setup(async (input) => {
    calls.push(input);
    return input.path === null
      ? root
      : { path: input.path, parent: "/allowed", directories: [], next: null };
  });
  const browser = client.projectsPresentation.directories.open("node");
  cleanup.push(async () => {
    browser.close();
  });
  const { signal } = new AbortController();
  expect(await browser.list({ path: null, cursor: null }, signal)).toEqual(root);
  expect(await browser.list({ path: "/allowed/project", cursor: "page" }, signal)).toMatchObject({
    path: "/allowed/project",
    parent: "/allowed",
  });
  expect(calls).toEqual([
    { path: null, cursor: null },
    { path: "/allowed/project", cursor: "page" },
  ]);
  expect(acquired.filter((service) => service === "swarmie.directories")).toHaveLength(1);
});

test("a late folder result cannot cross a replaced node session", async () => {
  const gate = Promise.withResolvers<unknown>();
  const entered = Promise.withResolvers<undefined>();
  const { client } = await setup(async () => {
    entered.resolve(undefined);
    return await gate.promise;
  });
  const pending = client.projectsPresentation.directories
    .open("node")
    .list({ path: null, cursor: null }, new AbortController().signal);
  const result = outcome(pending);
  await entered.promise;
  client.disconnect("node");
  gate.resolve(root);
  expect(await result).toBeInstanceOf(Error);
});

test("cancelling folder browsing rejects a pending read and does not add a project link", async () => {
  const gate = Promise.withResolvers<unknown>();
  const entered = Promise.withResolvers<undefined>();
  const { client } = await setup(async () => {
    entered.resolve(undefined);
    return await gate.promise;
  });
  const controller = new AbortController();
  const operation = client.projectsPresentation.directories
    .open("node")
    .list({ path: null, cursor: null }, controller.signal);
  const result = outcome(operation);
  await entered.promise;
  controller.abort();
  expect(await result).toMatchObject({ name: "AbortError" });
  expect(client.projectsState.getSnapshot().personalFolders).toEqual([]);
  gate.resolve(root);
});

test("malformed folder responses do not become selectable paths", () => {
  for (const value of [
    null,
    { ...root, parent: 3 },
    { ...root, directories: [{ name: "project" }] },
    { ...root, path: "\0" },
    { ...root, next: 7 },
  ]) {
    expect(() => directoryPage(value)).toThrow();
  }
});
