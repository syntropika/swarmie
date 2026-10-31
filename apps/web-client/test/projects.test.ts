import { afterEach, expect, test } from "bun:test";

import { projectsOrb } from "@swarmie/plugin-projects-browser";
import { decodeProjects, savedBrowserProjects } from "@swarmie/plugin-projects-browser/persistence";
import { ProjectsContext, ProjectsPage } from "@swarmie/plugin-projects-browser/views";
import { createHost } from "@syntropika/tembra-sdk/node";
import { sleep } from "bun";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { saveProjectChanges } from "../../../plugins/projects/browser/src/save";
import { fixtureProjects } from "../../../tests/fixtures/client-ui/projects";
import { initializeClient } from "../src/client/initialize";
import { fixtureNode, fixtureConnector } from "./connection-fixture";

let close: (() => Promise<void>) | undefined;
afterEach(async () => {
  await close?.();
  close = undefined;
});

async function start(backend = fixtureProjects()) {
  const host = createHost();
  const client = await initializeClient(host, {
    dev: true,
    projects: backend,
  });
  client.projectsState.update((current) => ({
    ...current,
    ...backend.snapshot(),
    available: true,
  }));
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
  const backend = fixtureProjects();
  const client = await start(backend);
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
  expect(backend.snapshot().personalFolders).toHaveLength(2);
  expect(backend.snapshot().personalFolders[0]?.paths).toEqual(["/source/website"]);
  await close?.();
  close = undefined;
  const restored = await start(backend);
  expect(restored.projectsState.getSnapshot().records[0]?.id).toBe(project.id);
  expect(restored.projects.projectOf(chatId)).toBe(project.id);
  await restored.projects.setPersonalFolders(project.id, { nodeId: "node-a", paths: [] });
  expect(restored.projectsState.getSnapshot().personalFolders).toEqual([
    { projectId: project.id, nodeId: "node-b", paths: ["/other/checkout"] },
  ]);
  expect(decodeProjects(backend.snapshot()).personalFolders).toEqual(
    backend.snapshot().personalFolders,
  );
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
    projects: fixtureProjects(),
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

test("saved Browser projects have an explicit import action and the source is retained", async () => {
  const original = Object.getOwnPropertyDescriptor(globalThis, "localStorage");
  const stored = JSON.stringify({
    records: [{ id: "legacy", name: "Existing", description: "", archived: false }],
    links: {},
    personalFolders: [],
  });
  Object.defineProperty(globalThis, "localStorage", {
    configurable: true,
    value: { getItem: () => stored },
  });
  try {
    const client = await start();
    const render = () =>
      renderToStaticMarkup(
        createElement(
          ProjectsContext,
          { value: { ...client.projectsPresentation, browserProfileKey: "saved" } },
          createElement(ProjectsPage, { params: {} }),
        ),
      );
    expect(render()).toContain("Import browser projects");
    const saved = savedBrowserProjects("saved");
    if (saved === null) {
      throw new Error("Fixture export is missing.");
    }
    await client.projects.importData(saved);
    expect(client.projectsState.getSnapshot().records[0]?.id).toBe("legacy");
    expect(savedBrowserProjects("saved")).toEqual(saved);
    expect(render()).not.toContain("Import browser projects");
  } finally {
    if (original) {
      Object.defineProperty(globalThis, "localStorage", original);
    } else {
      Reflect.deleteProperty(globalThis, "localStorage");
    }
  }
});

test("a confirmed project command remains successful when its subsequent projection read fails", async () => {
  const backend = fixtureProjects();
  backend.read = async () => await Promise.reject(new Error("Synthetic projection failure"));
  const client = await start(backend);
  const project = await client.projects.create({ name: "Confirmed" });
  expect(backend.snapshot().records[0]?.id).toBe(project.id);
  expect(client.projectsState.getSnapshot().error).toBe("Synthetic projection failure");
});

test("a retired editor save cannot write folders, close a new dialog or navigate in the replacement scope", async () => {
  const backend = fixtureProjects();
  const original = backend.change;
  const gate = Promise.withResolvers<undefined>();
  const writes: { scope: string | null; kind: string }[] = [];
  const client = await start(backend);
  backend.change = async (command) => {
    writes.push({ scope: client.projectsState.getSnapshot().nodeId, kind: command.kind });
    const receipt = await original(command);
    if (command.kind === "create") {
      await gate.promise;
    }
    return receipt;
  };
  await client.projectsState.commit((current) => ({ ...current, nodeId: "a" }));
  const navigated: string[] = [];
  const saving = saveProjectChanges(
    client.projects,
    {
      id: null,
      name: "First",
      description: "",
      folders: [{ projectId: "", nodeId: "a", paths: ["/checkout"] }],
    },
    (id) => {
      navigated.push(id);
    },
  );
  await sleep(5);
  const created = backend.snapshot().records;
  client.projectsState.update((current) => ({
    ...current,
    generation: current.generation + 1,
    nodeId: "b",
    records: created,
    dialog: { projectId: null },
  }));
  await sleep(5);
  gate.resolve(undefined);
  await saving;
  expect(writes).toEqual([{ scope: "a", kind: "create" }]);
  expect(navigated).toEqual([]);
  expect(client.projectsState.getSnapshot().dialog).toEqual({ projectId: null });
});

test("old draft admission preserves a newer project selection in the same or replacement scope", async () => {
  const backend = fixtureProjects();
  const client = await start(backend);
  const first = await client.projects.create({ name: "First" });
  const second = await client.projects.create({ name: "Second" });
  const original = backend.change;
  let gate = Promise.withResolvers<undefined>();
  backend.change = async (command) => {
    const receipt = await original(command);
    if (command.kind === "assign") {
      await gate.promise;
    }
    return receipt;
  };
  await client.projects.stageConversation("chat", first.id);
  const firstAdmission = client.projects.admitDraft("chat");
  await sleep(5);
  await client.projects.stageConversation("chat", second.id);
  gate.resolve(undefined);
  await firstAdmission;
  expect(client.projectsState.getSnapshot().draftLinks.chat).toBe(second.id);
  gate = Promise.withResolvers<undefined>();
  const secondAdmission = client.projects.admitDraft("chat");
  await sleep(5);
  client.projectsState.update((current) => ({
    ...current,
    generation: current.generation + 1,
    nodeId: "replacement",
    draftLinks: { chat: first.id },
  }));
  await sleep(5);
  gate.resolve(undefined);
  await secondAdmission;
  expect(client.projectsState.getSnapshot().draftLinks.chat).toBe(first.id);
});
