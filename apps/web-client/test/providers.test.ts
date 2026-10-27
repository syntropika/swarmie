import { afterEach, expect, test } from "bun:test";

import type { Snapshot } from "@swarmie/client-ui";
import type {
  ProviderAccount,
  ProviderAuthorizationState,
  ProvidersState,
} from "@swarmie/plugin-providers-browser";
import { createHost } from "@syntropika/tembra-sdk/node";
import { sleep } from "bun";

import { initializeClient } from "../src/client/initialize";

let close: (() => Promise<void>) | undefined;
afterEach(async () => {
  await close?.();
  close = undefined;
});
function source<T>(initial: T) {
  let current = initial;
  const listeners = new Set<() => void>();
  return {
    getSnapshot: () => current,
    subscribe(changed: () => void) {
      listeners.add(changed);
      return () => {
        listeners.delete(changed);
      };
    },
    publish(value: T) {
      current = value;
      for (const changed of listeners) {
        changed();
      }
    },
    listeners,
  };
}
const account: ProviderAccount = {
  id: "primary",
  providerId: "provider",
  providerLabel: "Provider",
  label: "Personal account",
  available: true,
  models: [
    { id: "balanced", label: "Balanced", efforts: ["Low", "Medium", "High"] },
    { id: "fast", label: "Fast", efforts: [] },
  ],
};
const catalog = (accounts: ProviderAccount[] = []): Readonly<ProvidersState> => ({
  choices: [],
  accounts,
  ready: true,
  error: null,
});

test("one Providers API configures an account and supplies its models without onboarding", async () => {
  const backend = source(catalog());
  const client = await initializeClient(createHost(), {
    dev: true,
    providers: {
      catalog: backend,
      configuration: { configure: async () => account },
    },
  });
  ({ close } = client);
  expect(client.providers.defaultSelection()).toBeNull();
  const { configuration } = client.providers;
  if (configuration === null) {
    throw new Error("Expected provider configuration.");
  }
  await configuration.configure(
    { providerId: "provider", method: "api-key", credential: "test-secret" },
    new AbortController().signal,
  );
  expect(client.providers.state.getSnapshot().accounts).toEqual([account]);
  expect(client.providers.defaultSelection()).toEqual({
    account: "primary",
    model: "balanced",
    effort: "Medium",
  });
  await sleep(10);
  expect(client.activeConversation().draft.getSnapshot().model).toBe("balanced");
  expect(
    client.plugins
      .getSnapshot()
      .filter((plugin) => ["models", "providers"].includes(plugin.id))
      .map((plugin) => plugin.id),
  ).toEqual(["providers"]);
  expect(JSON.stringify(client.providers.state.getSnapshot())).not.toContain("test-secret");
  await client.close();
  close = undefined;
  expect(backend.listeners.size).toBe(0);
});

test("provider selection stays account-scoped and never invents unsupported effort options", async () => {
  const backend = source(
    catalog([
      account,
      {
        ...account,
        id: "secondary",
        label: "Another account",
        models: [{ id: "other", label: "Other model", efforts: ["Low"] }],
      },
    ]),
  );
  const client = await initializeClient(createHost(), {
    dev: false,
    providers: { catalog: backend },
  });
  ({ close } = client);
  await sleep(10);
  const current = { account: "primary", model: "balanced", effort: "High" };
  expect(client.providers.selectModel(current, "fast")).toEqual({
    account: "primary",
    model: "fast",
    effort: "",
  });
  expect(client.providers.selectModel(current, "other")).toBeNull();
  expect(client.providers.selectAccount("secondary", current)).toEqual({
    account: "secondary",
    model: "other",
    effort: "Low",
  });
  const chat = client.activeConversation();
  chat.draft.update((draft) => ({ ...draft, ...current, text: "Keep my draft" }));
  backend.publish(catalog([]));
  await sleep(10);
  expect(chat.draft.getSnapshot()).toMatchObject({ ...current, text: "Keep my draft" });
  expect(client.providers.resolve(current).available).toBe(false);
  expect(client.providers.selectModel(current, "fast")).toBeNull();
});

test("Providers observes authorization results independently of the configuration screen and releases sessions", async () => {
  const authorization = source<ProviderAuthorizationState>({
    kind: "waiting",
    authorizationUrl: "https://example.com/authorize",
    userCode: null,
    callbackUri: null,
  });
  let disposed = 0;
  const backend: Snapshot<Readonly<ProvidersState>> = source(catalog());
  const client = await initializeClient(createHost(), {
    dev: false,
    providers: {
      catalog: backend,
      authorization: {
        begin: async () => ({
          ...authorization,
          complete: async () => undefined,
          cancel: async () => undefined,
          dispose: () => {
            disposed += 1;
          },
        }),
      },
    },
  });
  ({ close } = client);
  const authorizationApi = client.providers.authorization;
  if (authorizationApi === null) {
    throw new Error("Expected account authorization.");
  }
  expect(client.providers.configuration).toBeNull();
  expect(client.providers.discovery).toBeNull();
  const session = await authorizationApi.begin(
    { providerId: "provider", method: "oauth" },
    new AbortController().signal,
  );
  expect(authorization.listeners.size).toBe(1);
  authorization.publish({ kind: "connected", account });
  await sleep(10);
  expect(client.providers.defaultSelection()?.account).toBe("primary");
  expect(client.providers.state.getSnapshot().accounts).toEqual([account]);
  await client.close();
  close = undefined;
  expect(authorization.listeners.size).toBe(0);
  expect(disposed).toBe(1);
  session.dispose();
  expect(disposed).toBe(1);
  expect(
    authorizationApi.begin(
      { providerId: "provider", method: "oauth" },
      new AbortController().signal,
    ),
  ).rejects.toThrow("Providers is closed.");
});

test("discovery is independently available and exposes its complete contract", async () => {
  let received: AbortSignal | undefined;
  const client = await initializeClient(createHost(), {
    dev: false,
    providers: {
      catalog: source(catalog()),
      discovery: {
        detect: async (signal) => {
          received = signal;
          return ["http://localhost:11434"];
        },
      },
    },
  });
  ({ close } = client);
  expect(client.providers.configuration).toBeNull();
  expect(client.providers.authorization).toBeNull();
  const { discovery } = client.providers;
  if (discovery === null) {
    throw new Error("Expected local discovery.");
  }
  const controller = new AbortController();
  expect(await discovery.detect(controller.signal)).toEqual(["http://localhost:11434"]);
  expect(received).toBe(controller.signal);
});
