import { expect, test } from "bun:test";

import type {
  ModelDefaults,
  ProviderAccount,
  ProvidersState,
} from "@swarmie/plugin-providers-browser";
import { createHost } from "@syntropika/tembra-sdk/node";

import { initializeClient } from "../src/client/initialize";
import { modelDefaults } from "../src/client/provider-values";

const account: ProviderAccount = {
  id: "primary",
  providerId: "fixture",
  providerLabel: "Fixture",
  label: "Account",
  available: true,
  models: [
    { id: "small", label: "Small", efforts: [] },
    { id: "main", label: "Main", efforts: ["brief", "balanced", "deep"] },
  ],
};
function catalog() {
  let value: Readonly<ProvidersState> = {
    scope: null,
    choices: [],
    accounts: [account],
    ready: true,
    error: null,
  };
  const listeners = new Set<() => void>();
  return {
    getSnapshot: () => value,
    subscribe: (listener: () => void) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    publish: (next: Readonly<ProvidersState>) => {
      value = next;
      for (const listener of listeners) {
        listener();
      }
    },
  };
}

test("saved defaults affect new conversations and keep an existing draft's override", async () => {
  const source = catalog();
  const client = await initializeClient(createHost(), {
    dev: false,
    providers: {
      catalog: source,
      defaults: {
        save: async (_accountId, defaults) => {
          source.publish({ ...source.getSnapshot(), accounts: [{ ...account, defaults }] });
          return defaults;
        },
      },
    },
  });
  try {
    const existing = client.activeConversation();
    existing.draft.update((draft) => ({ ...draft, text: "Keep this draft" }));
    const defaults: ModelDefaults = { model: "main", effort: "deep", auxiliaryModel: "small" };
    await client.providers.defaults?.save(account.id, defaults, new AbortController().signal);
    expect(client.providers.defaultSelection()).toEqual({
      account: "primary",
      model: "main",
      effort: "deep",
    });
    expect(existing.draft.getSnapshot()).toMatchObject({ text: "Keep this draft", model: "small" });
    const next = client.createConversation();
    expect(next.draft.getSnapshot()).toMatchObject({
      account: "primary",
      model: "main",
      effort: "deep",
    });
    expect(client.providers.auxiliarySelection("primary")).toEqual({
      account: "primary",
      model: "small",
      effort: "",
    });
    await client.providers.defaults?.save(
      account.id,
      { ...defaults, auxiliaryModel: null },
      new AbortController().signal,
    );
    expect(client.providers.auxiliarySelection("primary")).toEqual(
      client.providers.defaultSelection(),
    );
  } finally {
    await client.close();
  }
});

test("a defaults response from an earlier node cannot update the current account", async () => {
  const source = catalog();
  const response = Promise.withResolvers<ModelDefaults>();
  const client = await initializeClient(createHost(), {
    dev: false,
    providers: { catalog: source, defaults: { save: async () => await response.promise } },
  });
  try {
    const defaults: ModelDefaults = { model: "main", effort: "deep", auxiliaryModel: null };
    const defaultsApi = client.providers.defaults;
    if (defaultsApi === null) {
      throw new Error("Expected writable defaults.");
    }
    const pending = defaultsApi.save(account.id, defaults, new AbortController().signal);
    source.publish({
      ...source.getSnapshot(),
      scope: { nodeId: "another-node", principal: "user", generation: 2 },
    });
    response.resolve(defaults);
    const message = await pending.then(
      () => "accepted",
      (error: unknown) => (error instanceof Error ? error.message : "unknown"),
    );
    expect(message).toContain("earlier node session");
    expect(client.providers.state.getSnapshot().accounts[0]?.defaults).toBeUndefined();
  } finally {
    await client.close();
  }
});

test("defaults validation retains exact provider reasoning names and rejects malformed responses", () => {
  expect(modelDefaults({ model: "main", effort: "balanced", auxiliaryModel: null })).toEqual({
    model: "main",
    effort: "balanced",
    auxiliaryModel: null,
  });
  for (const value of [
    {},
    { model: "main", effort: 4, auxiliaryModel: null },
    { model: null, effort: "", auxiliaryModel: null },
  ]) {
    expect(() => modelDefaults(value)).toThrow("invalid model defaults");
  }
});
