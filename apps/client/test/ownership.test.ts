import { afterEach, describe, expect, test } from "bun:test";

import type { ProvidersState } from "@swarmie/plugin-providers-browser";
import { createHost } from "@syntropika/tembra-sdk/node";
import { sleep } from "bun";

import type { ClientUi } from "../src/client/initialize";
import { initializeClient } from "../src/client/initialize";
import { fixtureNode, fixtureConnector } from "./connection-fixture";

let client: ClientUi;
let close: (() => Promise<void>) | undefined;
afterEach(async () => {
  await close?.();
  close = undefined;
});
async function start() {
  const host = createHost();
  client = await initializeClient(host, { dev: true });
  ({ close } = client);
  await Promise.resolve();
  return host;
}
async function settle() {
  await sleep(10);
}
describe("frontend ownership on the actual Tembra host", () => {
  test("draft and selection survive tab expansion, closing and panel movement", async () => {
    await start();
    const chat = client.activeConversation();
    chat.draft.update((draft) => ({ ...draft, text: "Keep this draft" }));
    await settle();
    client.openTab();
    await settle();
    client.expandView();
    await settle();
    expect(client.shell.getSnapshot().expanded).toBe(true);
    const conversationTab = client.shell
      .getSnapshot()
      .tabs.find((tab) => tab.kind === "conversation");
    expect(conversationTab).toBeDefined();
    if (!conversationTab) {
      throw new Error("Expected the conversation tab.");
    }
    client.closeTab(conversationTab.id);
    await settle();
    expect(client.activeConversation().draft.getSnapshot().text).toBe("Keep this draft");
    const [lastTab] = client.shell.getSnapshot().tabs;
    if (!lastTab) {
      throw new Error("Expected the remaining view tab.");
    }
    client.closeTab(lastTab.id);
    await settle();
    expect(client.shell.getSnapshot().tabs).toHaveLength(0);
    expect(client.shell.getSnapshot().conversationVisible).toBe(true);
    expect(client.activeConversation().id).toBe(chat.id);
  });
  test("releasing the optional voice orb removes its contributions without clearing a draft", async () => {
    const host = await start();
    client.activeConversation().draft.update((draft) => ({ ...draft, text: "Untouched" }));
    await settle();
    expect(client.conversationsUi.composer.actions.getSnapshot()).toHaveLength(1);
    client.togglePlugin("read-aloud");
    await settle();
    expect(client.conversationsUi.composer.actions.getSnapshot()).toHaveLength(0);
    expect(client.conversationsUi.messages.actions.getSnapshot()).toHaveLength(0);
    expect(client.activeConversation().draft.getSnapshot().text).toBe("Untouched");
    client.togglePlugin("read-aloud");
    await settle();
    expect(client.conversationsUi.composer.actions.getSnapshot()).toHaveLength(1);
    expect(host.errors()).toEqual([]);
  });
  test("backend projections are separate from onboarding and unknown accounts remain visible", async () => {
    await start();
    client.providers.state.update((state) => ({
      ...state,
      ready: true,
      accounts: [
        {
          id: "account",
          providerId: "openai",
          providerLabel: "ChatGPT",
          label: "Saved account",
          models: [],
          available: false,
        },
      ],
    }));
    await settle();
    expect(client.providers.state.getSnapshot().accounts[0]?.label).toBe("Saved account");
    expect(client.providers.state.getSnapshot().accounts[0]?.available).toBe(false);
    expect(client.conversationState.getSnapshot().records).toHaveLength(1);
    expect(client.plugins.getSnapshot().some((plugin) => plugin.id === "app-state")).toBe(true);
  });
});

test("a subscribed provider catalog selects a model and preserves unavailable account identity", async () => {
  let catalog: Readonly<ProvidersState> = { ready: true, accounts: [], choices: [], error: null };
  const listeners = new Set<() => void>();
  client = await initializeClient(createHost(), {
    dev: false,
    providers: {
      catalog: {
        getSnapshot: () => catalog,
        subscribe: (changed) => {
          listeners.add(changed);
          return () => {
            listeners.delete(changed);
          };
        },
      },
    },
  });
  ({ close } = client);
  const chat = client.activeConversation();
  catalog = {
    ...catalog,
    accounts: [
      {
        id: "account",
        providerId: "provider",
        providerLabel: "Provider",
        label: "Saved account",
        available: true,
        models: [{ id: "model", label: "Model", efforts: ["Medium"] }],
      },
    ],
  };
  for (const notify of listeners) {
    notify();
  }
  await settle();
  expect(chat.draft.getSnapshot().account).toBe("account");
  expect(chat.draft.getSnapshot().model).toBe("model");
  chat.draft.update((draft) => ({ ...draft, text: "Keep this draft" }));
  catalog = { ...catalog, accounts: [] };
  for (const notify of listeners) {
    notify();
  }
  await settle();
  expect(chat.draft.getSnapshot().account).toBe("account");
  expect(chat.draft.getSnapshot().text).toBe("Keep this draft");
  expect(client.canSend(chat)).toBe(false);
  await close?.();
  close = undefined;
  expect(listeners.size).toBe(0);
});

test("an unknown admission is never replayed and later typing remains in the composer", async () => {
  let submissions = 0;
  const host = createHost();
  client = await initializeClient(host, {
    dev: true,
    nodes: [fixtureNode],
    connector: fixtureConnector,
    execution: () => ({
      submit: async () => {
        submissions += 1;
        await sleep(10);
        throw new Error("Connection lost after admission.");
      },
      stop: async () => undefined,
    }),
  });
  ({ close } = client);
  await settle();
  const chat = client.activeConversation();
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
    text: "Send this",
    account: "account",
    model: "model",
  }));
  await settle();
  client.send(chat);
  client.send(chat);
  chat.draft.update((draft) => ({ ...draft, text: "Keep this next thought" }));
  await sleep(25);
  expect(submissions).toBe(1);
  expect(chat.execution.getSnapshot().phase).toBe("unknown");
  expect(chat.draft.getSnapshot().text).toBe("Keep this next thought");
  expect(client.canSend(chat)).toBe(false);
});
