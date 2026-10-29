import { afterEach, expect, test } from "bun:test";

import { emptyProjects, projectsOrb } from "@swarmie/plugin-projects-browser";
import type { ProjectsData, ProjectsPersistence } from "@swarmie/plugin-projects-browser";
import { decodeProjects } from "@swarmie/plugin-projects-browser/persistence";
import { createHost } from "@syntropika/tembra-sdk/node";
import { sleep } from "bun";

import { initializeClient } from "../src/client/initialize";
import { fixtureNode, fixtureConnector } from "./connection-fixture";

let close: (() => Promise<void>) | undefined;
afterEach(async () => {
  await close?.();
  close = undefined;
});

async function start(persistence?: ProjectsPersistence) {
  const host = createHost();
  const client = await initializeClient(host, {
    dev: true,
    ...(persistence ? { projectsPersistence: persistence } : {}),
  });
  ({ close } = client);
  return client;
}

test("Projects core activates without Conversations or a shell", async () => {
  const host = createHost();
  close = async () => {
    await host.close();
  };
  await host.install(projectsOrb());
  const reference = await host.get("swarmie.projects.ui", "main");
  const value: unknown = reference.state.value;
  expect(value).toMatchObject({ records: [], links: {}, personalFolders: [] });
  expect(host.installedOrbs().map((entry) => entry.name)).toEqual(["swarmie.projects.ui"]);
  reference.release();
});

test("Projects owns stable associations without changing conversation data or its draft", async () => {
  const client = await start();
  const chat = client.activeConversation();
  chat.draft.update((current) => ({ ...current, text: "Keep this draft" }));
  const first = await client.projects.create({ name: "Website" });
  const second = await client.projects.create({ name: "Website" });
  expect(first.id).not.toBe(second.id);
  expect("nodeId" in first).toBe(false);
  await client.projects.assignConversation({ conversationId: chat.id, projectId: first.id });
  expect(client.projects.projectOf(chat.id)).toBe(first.id);
  expect("project" in chat).toBe(false);
  expect("projects" in client.conversationState.getSnapshot()).toBe(false);
  await client.projects.update(first.id, { name: "Renamed", description: "Project details" });
  expect(client.projects.projectOf(chat.id)).toBe(first.id);
  expect(client.projects.listConversations(first.id, { limit: 10 }).items).toEqual([chat.id]);
  await client.projects.assignConversation({ conversationId: chat.id, projectId: second.id });
  expect(client.projects.listConversations(first.id, { limit: 10 }).items).toEqual([]);
  expect(chat.draft.getSnapshot().text).toBe("Keep this draft");
  await client.projects.unassignConversation(chat.id);
  expect(client.projects.projectOf(chat.id)).toBe(null);
  expect(chat.draft.getSnapshot().text).toBe("Keep this draft");
});

test("personal folder links remain per node and project data reopens through its owner persistence", async () => {
  let saved = emptyProjects();
  const persistence = {
    load: () => saved,
    save: (data: ProjectsData) => {
      saved = data;
    },
  };
  const client = await start(persistence);
  const project = await client.projects.create({ name: "Website" });
  const chatId = client.activeConversation().id;
  await client.projects.assignConversation({ conversationId: chatId, projectId: project.id });
  await client.projects.setPersonalFolders(project.id, {
    nodeId: "node-a",
    paths: ["/source/website", "/source/website"],
  });
  await client.projects.setPersonalFolders(project.id, {
    nodeId: "node-b",
    paths: ["/other/checkout"],
  });
  expect(saved.personalFolders).toHaveLength(2);
  expect(saved.personalFolders[0]?.paths).toEqual(["/source/website"]);
  await close?.();
  close = undefined;
  const restored = await start(persistence);
  expect(restored.projectsState.getSnapshot().records[0]?.id).toBe(project.id);
  expect(restored.projects.projectOf(chatId)).toBe(project.id);
  await restored.projects.setPersonalFolders(project.id, { nodeId: "node-a", paths: [] });
  expect(restored.projectsState.getSnapshot().personalFolders).toEqual([
    { projectId: project.id, nodeId: "node-b", paths: ["/other/checkout"] },
  ]);
  expect(decodeProjects(saved)).toEqual(saved);
});

test("disabling Projects withdraws its section, header, sidebar and composer contributions but retains data", async () => {
  const client = await start();
  const project = await client.projects.create({ name: "Website" });
  const chat = client.activeConversation();
  await client.projects.assignConversation({ conversationId: chat.id, projectId: project.id });
  client.projectsPresentation.open(project.id);
  await sleep(15);
  expect(client.shell.getSnapshot().section).toBe("projects");
  expect(client.conversationsUi.composer.topBar.items.getSnapshot()).toHaveLength(1);
  expect(
    client.conversationsUi.navigation.groups.getSnapshot()[0]?.membership.getSnapshot(),
  ).toEqual([chat.id]);
  client.togglePlugin("projects");
  await sleep(20);
  expect(client.shellUi.sections.getSnapshot()).toHaveLength(0);
  expect(client.conversationsUi.composer.topBar.items.getSnapshot()).toHaveLength(0);
  expect(client.conversationsUi.navigation.groups.getSnapshot()).toHaveLength(0);
  expect(client.conversationsUi.header.items.getSnapshot()).toHaveLength(0);
  expect(client.shell.getSnapshot().section).toBe("conversations");
  expect(client.projects.projectOf(chat.id)).toBe(project.id);
  client.togglePlugin("projects");
  await sleep(20);
  expect(client.shellUi.sections.getSnapshot()).toHaveLength(1);
  expect(client.conversationsUi.composer.topBar.items.getSnapshot()).toHaveLength(1);
  expect(client.projects.projectOf(chat.id)).toBe(project.id);
});

test("project destinations participate in back and forward without opening view tabs", async () => {
  const client = await start();
  const project = await client.projects.create({ name: "Website" });
  client.projectsPresentation.open();
  await sleep(5);
  client.projectsPresentation.open(project.id);
  await sleep(5);
  expect(client.shell.getSnapshot().params).toEqual({ projectId: project.id });
  expect(client.views.getSnapshot().tabs).toHaveLength(0);
  client.travel(-1);
  await sleep(5);
  expect(client.shell.getSnapshot().section).toBe("projects");
  expect(client.shell.getSnapshot().params).toEqual({});
  client.travel(1);
  await sleep(5);
  expect(client.shell.getSnapshot().params).toEqual({ projectId: project.id });
});

test("new conversation is visual until admission, and Projects keeps its selection outside saved links", async () => {
  const admission = Promise.withResolvers<{ accepted: true }>();
  const creates: boolean[] = [];
  const client = await initializeClient(createHost(), {
    dev: true,
    nodes: [fixtureNode],
    connector: fixtureConnector,
    execution: () => ({
      submit: async (input) => {
        creates.push(input.createConversation);
        return await admission.promise;
      },
      stop: async () => undefined,
    }),
  });
  ({ close } = client);
  const project = await client.projects.create({ name: "Website" });
  await client.projectsPresentation.createConversation(project.id);
  const chat = client.activeConversation();
  expect(chat.started).toBe(false);
  expect(client.roster.getSnapshot().filter((entry) => entry.started)).toHaveLength(0);
  expect(client.projects.projectOf(chat.id)).toBe(null);
  expect(client.projectsState.getSnapshot().draftLinks[chat.id]).toBe(project.id);
  client.providers.state.update((state) => ({
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
  await sleep(10);
  client.send(chat);
  await sleep(5);
  expect(chat.started).toBe(false);
  expect(creates).toEqual([true]);
  admission.resolve({ accepted: true });
  await sleep(20);
  expect(chat.started).toBe(true);
  expect(client.roster.getSnapshot().filter((entry) => entry.started)).toHaveLength(1);
  expect(client.projects.projectOf(chat.id)).toBe(project.id);
  expect(client.projectsState.getSnapshot().draftLinks[chat.id]).toBeUndefined();
  chat.draft.update((draft) => ({ ...draft, text: "Second message" }));
  await sleep(10);
  client.send(chat);
  await sleep(10);
  expect(creates).toEqual([true, false]);
});

test("closing the mobile overlay in the same turn does not overwrite a new destination", async () => {
  const client = await start();
  client.navigate({ section: "projects", params: { projectId: "project" } });
  client.shellState.update((current) => ({ ...current, navigation: false }));
  await sleep(10);
  expect(client.shell.getSnapshot().section).toBe("projects");
  expect(client.shell.getSnapshot().params).toEqual({ projectId: "project" });
  expect(client.shell.getSnapshot().navigation).toBe(false);
});

test("node folder paths keep significant trailing spaces when saved", async () => {
  const client = await start();
  const project = await client.projects.create({ name: "Folder paths" });
  await client.projects.setPersonalFolders(project.id, {
    nodeId: "node-a",
    paths: ["/source/with trailing space "],
  });
  expect(client.projectsState.getSnapshot().personalFolders[0]?.paths).toEqual([
    "/source/with trailing space ",
  ]);
});
