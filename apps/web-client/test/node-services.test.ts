import { afterEach, expect, test } from "bun:test";

import { NodeProtocol } from "@swarmie/plugin-client-node-browser";
import type {
  ConversationExecutionApi,
  ConversationExecutionContext,
  Message,
} from "@swarmie/plugin-conversations-browser";
import type {
  ProviderAccount,
  ProviderAuthorizationState,
} from "@swarmie/plugin-providers-browser";
import { orb } from "@syntropika/tembra-sdk";
import type { Context, State } from "@syntropika/tembra-sdk";
import { createHost } from "@syntropika/tembra-sdk/node";

import { initializeClient } from "../src/client/initialize";

const cleanup: (() => Promise<void>)[] = [];
async function closeAll(): Promise<void> {
  const close = cleanup.pop();
  if (close !== undefined) {
    await close();
    await closeAll();
  }
}
afterEach(closeAll);
const account: ProviderAccount = {
  id: "fixture-account",
  providerId: "openai",
  providerLabel: "OpenAI",
  label: "Fixture account",
  available: true,
  models: [{ id: "fixture-model", label: "Fixture model", efforts: ["low", "high"] }],
};
const choices = [
  {
    id: "openai",
    label: "OpenAI",
    publisher: "OpenAI",
    methods: [{ id: "oauth-browser", label: "Continue with ChatGPT", kind: "oauth" }],
  },
];
type Input = Parameters<ConversationExecutionApi["submit"]>[0];
type Projection = {
  conversationId: string;
  ownerConversationId?: string | undefined;
  messages: Message[];
  phase: "idle" | "working" | "completed" | "failed" | "unknown";
  label: string;
  admitted: string[];
  omittedMessages?: number;
  canAcknowledge?: boolean;
  executionContext?: ConversationExecutionContext;
  lifecycleWarning?: string;
};

async function setup(
  configure: () => Promise<ProviderAccount> = async () => account,
  retryCatalog = false,
  options: {
    ownerConversationId?: string;
    refuse?: string;
    lifecycleWarning?: string;
    failBeforeAdmission?: boolean;
    finishPhase?: "completed" | "failed" | "unknown";
    catalog?: {
      id: string;
      title: string;
      account: string;
      model: string;
      effort: string;
      archived: boolean;
      pinned: boolean;
      executionContext?: ConversationExecutionContext;
    }[];
    providerPages?: ProviderAccount[][];
    completeAccount?: ProviderAccount;
    truncate?: boolean;
    beforeAdmission?: Promise<void>;
    beforeCatalog?: () => Promise<void>;
    beforeMetadata?: (title: string) => Promise<void>;
    beforeCatalogGet?: (count: number) => Promise<void>;
  } = {},
) {
  const backend = createHost();
  cleanup.push(async () => {
    await backend.close();
  });
  const finish = Promise.withResolvers<undefined>();
  const submissions: Input[] = [];
  const pages: (string | null)[] = [];
  const providerCursors: (string | null)[] = [];
  const metadata: { id: string; title: string; pinned: boolean; archived: boolean }[] = [];
  const observed: string[] = [];
  const catalog = structuredClone(options.catalog ?? []);
  let catalogGets = 0;
  let configured: ProviderAccount[] = [];
  let acknowledgements = 0;
  const authorizationKeys: (readonly string[])[] = [];
  await backend.install(
    orb({
      name: "fixture.node-services",
      apiVersion: 1,
      services: {
        "swarmie.providers.settings": {
          // The fixture catalogs survive individual consumer References and reconnects.
          linger: 30_000,
          create: () => undefined,
          methods: {
            list: (_state, _context, cursor: string | null) => {
              providerCursors.push(cursor);
              const index = cursor === null ? 0 : Number(cursor);
              const chunk = options.providerPages?.[index] ?? configured;
              return {
                choices,
                accounts: chunk,
                nextCursor:
                  options.providerPages !== undefined && index < options.providerPages.length - 1
                    ? String(index + 1)
                    : null,
              };
            },
            configure: async () => {
              const value = await configure();
              configured = [value];
              return value;
            },
          },
        },
        "swarmie.providers.authorization": {
          create: (self) => {
            authorizationKeys.push(self.key);
            return self.state<ProviderAuthorizationState>({
              kind: "waiting",
              authorizationUrl: "https://example.com/authorize",
              userCode: null,
              callbackUri: "http://localhost:1455/auth/callback",
            });
          },
          methods: {
            poll: () => undefined,
            complete: (
              state: State<ProviderAuthorizationState>,
              context: Context,
              callbackUrl: string,
            ) => {
              expect(callbackUrl).toStartWith("http://localhost:1455/auth/callback?");
              if (!retryCatalog) {
                configured = [options.completeAccount ?? account];
              }
              return state.update(context, () =>
                retryCatalog
                  ? {
                      kind: "failed",
                      error: "The model catalog exceeds the response limit",
                      retryable: true,
                    }
                  : { kind: "connected", account: options.completeAccount ?? account },
              );
            },
            retry: (state: State<ProviderAuthorizationState>, context: Context) => {
              configured = [account];
              return state.update(context, () => ({ kind: "connected", account }));
            },
            cancel: (state: State<ProviderAuthorizationState>, context: Context) =>
              state.update(context, () => ({ kind: "failed", error: "Cancelled" })),
          },
        },
        "swarmie.conversation.catalog": {
          linger: 30_000,
          create: () => undefined,
          methods: {
            list: async (_state, _context, cursor: string | null) => {
              await options.beforeCatalog?.();
              pages.push(cursor);
              const offset = cursor === null ? 0 : Number(cursor);
              const entries = catalog.slice(offset, offset + 32);
              return {
                entries,
                nextCursor:
                  offset + entries.length < catalog.length ? String(offset + entries.length) : null,
              };
            },
            update: async (
              _state,
              _context,
              id: string,
              title: string,
              pinned: boolean,
              archived: boolean,
            ) => {
              await options.beforeMetadata?.(title);
              const entry = catalog.find((value) => value.id === id);
              if (entry === undefined) {
                throw new Error("Conversation not indexed");
              }
              Object.assign(entry, { title, pinned, archived });
              metadata.push({ id, title, pinned, archived });
              return { ...entry, lifecycleWarning: options.lifecycleWarning ?? null };
            },
          },
        },
        "swarmie.conversation.session": {
          create: (self) =>
            self.state<Projection>({
              conversationId: self.key[0] ?? "",
              ownerConversationId: options.ownerConversationId,
              messages: [],
              phase: "idle",
              label: "",
              admitted: [],
              ...(() => {
                const entry = catalog.find((item) => item.id === self.key[0]);
                return entry?.executionContext ? { executionContext: entry.executionContext } : {};
              })(),
            }),
          methods: {
            snapshot: (state: State<Projection>) => {
              observed.push(state.value.conversationId);
            },
            async run(state: State<Projection>, context: Context, input: Input) {
              submissions.push(input);
              await options.beforeAdmission;
              if (options.refuse !== undefined) {
                return { status: "refused", reason: options.refuse };
              }
              if (options.failBeforeAdmission === true) {
                throw new Error("The transport cannot confirm admission.");
              }
              if (!catalog.some((entry) => entry.id === input.conversationId)) {
                catalog.push({
                  id: input.conversationId,
                  title: input.draft.text,
                  account: input.draft.account ?? "",
                  model: input.draft.model,
                  effort: input.draft.effort,
                  pinned: false,
                  archived: false,
                });
              }
              state.update(context, (value) => ({
                ...value,
                admitted: [input.submissionId],
                ...(input.executionContext ? { executionContext: input.executionContext } : {}),
                phase: "working",
                canAcknowledge: false,
                messages: [{ id: "user", role: "user", text: input.draft.text }],
              }));
              await finish.promise;
              state.update(context, (value) => ({
                ...value,
                phase: options.finishPhase ?? "completed",
                canAcknowledge: options.finishPhase === "unknown",
                omittedMessages: options.truncate === true ? 1000 : 0,
                messages: [
                  ...value.messages,
                  {
                    id: "assistant",
                    role: "assistant",
                    text: "Done",
                    ...(options.truncate === true ? { truncated: true } : {}),
                  },
                ],
              }));
              return { status: "settled" };
            },
            acknowledge: (state: State<Projection>, context: Context) => {
              acknowledgements += 1;
              state.update(context, (value) => ({
                ...value,
                phase: "failed",
                canAcknowledge: false,
                label: "Unknown tool outcome acknowledged. No tool was replayed.",
              }));
            },
          },
        },
      },
    }),
  );
  const client = await initializeClient(createHost(), {
    dev: false,
    nodeServices: true,
    nodes: [{ id: "fixture-node", label: "Fixture node", principal: "fixture-user", url: null }],
    connector: async (node) => ({
      identity: {
        nodeId: node.id,
        principal: node.principal,
        protocolVersion: NodeProtocol.Version,
      },
      get: async (service, key) => {
        if (service === "swarmie.conversation.catalog") {
          catalogGets += 1;
          await options.beforeCatalogGet?.(catalogGets);
        }
        return await backend.get(service, key, { principal: node.principal });
      },
      close: () => undefined,
    }),
  });
  cleanup.push(client.close, async () => {
    finish.resolve(undefined);
    await Promise.resolve(undefined);
  });
  await client.nodeConnections.open("fixture-node");
  await Bun.sleep(20);
  return {
    client,
    finish,
    submissions,
    pages,
    providerCursors,
    authorizationKeys,
    metadata,
    observed,
    acknowledgements: () => acknowledgements,
  };
}

test("a verified node loads provider choices, triggers first-use onboarding and observes OAuth accounts", async () => {
  const { client } = await setup();
  expect(client.providers.state.getSnapshot()).toMatchObject({
    ready: true,
    choices,
    accounts: [],
  });
  expect(client.onboarding.getSnapshot().open).toBe(true);
  const { authorization } = client.providers;
  if (authorization === null) {
    throw new Error("Expected node authorization.");
  }
  const controller = new AbortController();
  const session = await authorization.begin(
    { providerId: "openai", method: "oauth-browser" },
    controller.signal,
  );
  expect(session.getSnapshot().kind).toBe("waiting");
  await session.complete("http://localhost:1455/auth/callback?code=fixture");
  await Bun.sleep(10);
  expect(client.providers.state.getSnapshot().accounts).toEqual([
    { ...account, modelsTruncated: false },
  ]);
  expect(client.providers.defaultSelection()).toMatchObject({
    account: account.id,
    model: "fixture-model",
  });
  let notifications = 0;
  const detach = session.subscribe(() => {
    notifications += 1;
  });
  controller.abort();
  expect(session.getSnapshot().kind).toBe("failed");
  expect(notifications).toBeGreaterThan(0);
  detach();
  session.dispose();
});

test("a late configuration result cannot populate a disconnected node session", async () => {
  const gate = Promise.withResolvers<ProviderAccount>();
  const entered = Promise.withResolvers<undefined>();
  const { client } = await setup(async () => {
    entered.resolve(undefined);
    return await gate.promise;
  });
  const { configuration } = client.providers;
  if (configuration === null) {
    throw new Error("Expected node configuration.");
  }
  const operation = configuration.configure(
    { providerId: "openai", method: "api-key", credential: "fixture-secret" },
    new AbortController().signal,
  );
  async function outcome() {
    try {
      await operation;
      return null;
    } catch (error: unknown) {
      return error;
    }
  }
  const result = outcome();
  await entered.promise;
  client.disconnect("fixture-node");
  gate.resolve(account);
  expect(await result).toBeInstanceOf(Error);
  expect(client.providers.state.getSnapshot().accounts).toEqual([]);
});

test("a post-authorization catalog failure can retry the same session and publish its account", async () => {
  const { client } = await setup(undefined, true);
  const session = await client.providers.authorization?.begin(
    { providerId: "openai", method: "oauth-browser" },
    new AbortController().signal,
  );
  if (session === undefined) {
    throw new Error("Expected node authorization.");
  }
  await session.complete("http://localhost:1455/auth/callback?code=fixture");
  expect(session.getSnapshot()).toMatchObject({ kind: "failed", retryable: true });
  expect(client.providers.state.getSnapshot().accounts).toEqual([]);
  if (session.retry === undefined) {
    throw new Error("Expected authorization retry.");
  }
  await session.retry();
  await Bun.sleep(10);
  expect(session.getSnapshot().kind).toBe("connected");
  expect(client.providers.state.getSnapshot().accounts).toEqual([
    { ...account, modelsTruncated: false },
  ]);
  session.dispose();
});

test("node admission clears the sent draft while execution remains observed until completion", async () => {
  const { client, finish, submissions } = await setup();
  await client.providers.configuration?.configure(
    { providerId: "openai", method: "api-key", credential: "fixture-secret" },
    new AbortController().signal,
  );
  const chat = client.activeConversation();
  chat.draft.update((value) => ({
    ...value,
    text: "Hello",
    account: account.id,
    model: "fixture-model",
    effort: "low",
  }));
  await Bun.sleep(10);
  expect(client.canSend(chat)).toBe(true);
  client.send(chat);
  await Bun.sleep(20);
  expect(submissions).toHaveLength(1);
  expect(chat.started).toBe(true);
  expect(chat.draft.getSnapshot().text).toBe("");
  expect(chat.execution.getSnapshot().phase).toBe("working");
  finish.resolve(undefined);
  await Bun.sleep(20);
  expect(chat.execution.getSnapshot().phase).toBe("completed");
  expect(chat.messages.getSnapshot().at(-1)?.text).toBe("Done");
});

async function readyDraft(client: Awaited<ReturnType<typeof setup>>["client"]) {
  await client.providers.configuration?.configure(
    { providerId: "openai", method: "api-key", credential: "fixture-secret" },
    new AbortController().signal,
  );
  const chat = client.activeConversation();
  chat.draft.update((value) => ({
    ...value,
    text: "Keep this draft",
    account: account.id,
    model: "fixture-model",
    effort: "low",
  }));
  await Bun.sleep(10);
  return chat;
}

async function until(check: () => boolean, remaining = 100): Promise<void> {
  if (check()) {
    return;
  }
  if (remaining === 0) {
    expect(check()).toBe(true);
    return;
  }
  await Bun.sleep(2);
  await until(check, remaining - 1);
}

test("metadata edits retain their order when catalog acquisition is delayed", async () => {
  const delayed = Promise.withResolvers<undefined>();
  const { client, finish, metadata } = await setup(undefined, false, {
    beforeCatalogGet: async (count) => {
      if (count === 2) {
        await delayed.promise;
      }
    },
  });
  const chat = await readyDraft(client);
  finish.resolve(undefined);
  client.send(chat);
  await until(() => chat.execution.getSnapshot().phase === "completed");
  client.renameConversation(chat, "Latest title");
  client.togglePin(chat);
  await Bun.sleep(10);
  delayed.resolve(undefined);
  await until(() => metadata.length === 2);
  expect(metadata.at(-1)).toMatchObject({ title: "Latest title", pinned: true });
});

test("metadata edits do not reopen connections or depend on open promise order", async () => {
  const { client, finish, metadata } = await setup();
  const chat = await readyDraft(client);
  finish.resolve(undefined);
  client.send(chat);
  await until(() => chat.execution.getSnapshot().phase === "completed");
  const open = client.nodeConnections.open.bind(client.nodeConnections);
  const delayed = Promise.withResolvers<undefined>();
  let opens = 0;
  client.nodeConnections.open = async (id) => {
    opens += 1;
    if (opens === 1) {
      await delayed.promise;
    }
    return await open(id);
  };
  client.renameConversation(chat, "Ordered title");
  client.togglePin(chat);
  await Bun.sleep(10);
  delayed.resolve(undefined);
  await until(() => metadata.length === 2);
  expect(opens).toBe(0);
  expect(metadata[0]).toMatchObject({ title: "Ordered title", pinned: false });
  expect(metadata[1]).toMatchObject({ title: "Ordered title", pinned: true });
});

test("a queued metadata edit cannot reach a replacement session before restore", async () => {
  const acquisition = Promise.withResolvers<undefined>();
  const restoring = Promise.withResolvers<undefined>();
  let catalogs = 0;
  const { client, finish, metadata } = await setup(undefined, false, {
    beforeCatalogGet: async (count) => {
      if (count === 2) {
        await acquisition.promise;
      }
    },
    beforeCatalog: async () => {
      catalogs += 1;
      if (catalogs > 1) {
        await restoring.promise;
      }
    },
  });
  cleanup.push(async () => {
    acquisition.resolve(undefined);
    restoring.resolve(undefined);
  });
  const chat = await readyDraft(client);
  finish.resolve(undefined);
  client.send(chat);
  await until(() => chat.execution.getSnapshot().phase === "completed");
  chat.draft.update((value) => ({ ...value, text: "Next request" }));
  client.renameConversation(chat, "Old session edit");
  await Bun.sleep(5);
  client.disconnect("fixture-node");
  await client.nodeConnections.open("fixture-node");
  acquisition.resolve(undefined);
  await until(() => client.conversationError.getSnapshot()?.includes("ended") === true);
  expect(client.canSend(chat)).toBe(false);
  expect(metadata).toHaveLength(0);
  restoring.resolve(undefined);
  await until(() => client.canSend(chat));
  expect(metadata).toHaveLength(0);
});

test("rapid metadata toggles use confirmed values and do not repeat a rejected title", async () => {
  const attempts: string[] = [];
  const rejected = "é".repeat(257);
  const { client, finish, metadata } = await setup(undefined, false, {
    beforeMetadata: async (title) => {
      attempts.push(title);
      if (new TextEncoder().encode(title).length > 512) {
        throw new Error("Metadata save refused.");
      }
    },
  });
  const chat = await readyDraft(client);
  finish.resolve(undefined);
  client.send(chat);
  await until(() => chat.execution.getSnapshot().phase === "completed");
  const confirmedTitle = chat.title;
  client.togglePin(chat);
  client.togglePin(chat);
  await until(() => metadata.length === 2);
  expect(chat.pinned).toBe(false);
  expect(metadata.at(-1)?.pinned).toBe(false);
  client.renameConversation(chat, rejected);
  await until(() => client.conversationError.getSnapshot() === "Metadata save refused.");
  client.togglePin(chat);
  await until(() => metadata.length === 3);
  expect(metadata.at(-1)).toMatchObject({ title: confirmedTitle, pinned: true });
  expect(chat.title).toBe(confirmedTitle);
  expect(attempts.filter((title) => title === rejected)).toHaveLength(1);
});

test("a pending run retains its observation when a ninth conversation opens", async () => {
  const admission = Promise.withResolvers<undefined>();
  const catalog = Array.from({ length: 9 }, (_, index) => ({
    id: `observed-${index}`,
    title: `Observed ${index}`,
    account: account.id,
    model: "fixture-model",
    effort: "low",
    archived: false,
    pinned: false,
  }));
  const { client, finish, submissions, observed } = await setup(undefined, false, {
    catalog,
    beforeAdmission: admission.promise,
  });
  await readyDraft(client);
  client.openConversation("observed-0");
  await until(() => observed.includes("observed-0"));
  for (const entry of catalog.slice(1, 8)) {
    client.openConversation(entry.id);
  }
  await until(() => observed.length === 8);
  const chat = client.roster.getSnapshot().find((entry) => entry.id === "observed-0");
  if (!chat) {
    throw new Error("Expected the first observed conversation.");
  }
  chat.draft.update((draft) => ({ ...draft, text: "Keep observing" }));
  await until(() => client.canSend(chat));
  client.send(chat);
  await until(() => submissions.length === 1);
  client.openConversation("observed-8");
  await until(() => observed.includes("observed-8"));
  admission.resolve(undefined);
  finish.resolve(undefined);
  await until(() => chat.messages.getSnapshot().at(-1)?.text === "Done");
  expect(chat.execution.getSnapshot().phase).toBe("completed");
});

test("empty catalog restoration notifies availability without changing execution state", async () => {
  const catalog = Promise.withResolvers<undefined>();
  const { client } = await setup(undefined, false, {
    beforeCatalog: async () => {
      await catalog.promise;
    },
  });
  const chat = await readyDraft(client);
  const execution = chat.execution.getSnapshot();
  let changes = 0;
  const detach = client.subscribeExecution(() => {
    changes += 1;
  });
  expect(client.canSend(chat)).toBe(false);
  catalog.resolve(undefined);
  await until(() => client.canSend(chat));
  expect(chat.execution.getSnapshot()).toBe(execution);
  expect(changes).toBeGreaterThan(0);
  detach();
});

test("a new connection generation waits for its own conversation catalog", async () => {
  let catalog = Promise.withResolvers<undefined>();
  catalog.resolve(undefined);
  const { client } = await setup(undefined, false, {
    beforeCatalog: async () => {
      await catalog.promise;
    },
  });
  const chat = await readyDraft(client);
  expect(client.canSend(chat)).toBe(true);
  client.disconnect("fixture-node");
  expect(client.canSend(chat)).toBe(false);
  catalog = Promise.withResolvers<undefined>();
  await client.connect("fixture-node");
  await until(() => client.providers.state.getSnapshot().ready);
  const premature = client.canSend(chat);
  catalog.resolve(undefined);
  expect(premature).toBe(false);
  await until(() => client.canSend(chat));
});

test("a typed native refusal preserves the draft and releases its pre-admission destination", async () => {
  const { client, submissions } = await setup(undefined, false, {
    refuse: "The account needs authorization.",
  });
  const chat = await readyDraft(client);
  client.send(chat);
  await Bun.sleep(20);
  expect(submissions).toHaveLength(1);
  expect(chat.started).toBe(false);
  expect(chat.node).toBeNull();
  expect(chat.draft.getSnapshot().text).toBe("Keep this draft");
  expect(chat.execution.getSnapshot().phase).toBe("idle");
  expect(client.canSend(chat)).toBe(true);
  expect(client.conversationError.getSnapshot()).toBe("The account needs authorization.");
});

test("an untyped admission failure keeps its destination bound and never resends automatically", async () => {
  const { client, submissions, acknowledgements } = await setup(undefined, false, {
    failBeforeAdmission: true,
  });
  const chat = await readyDraft(client);
  client.send(chat);
  await Bun.sleep(20);
  expect(submissions).toHaveLength(1);
  expect(chat.node).toBe("fixture-node");
  expect(chat.draft.getSnapshot().text).toBe("Keep this draft");
  expect(chat.execution.getSnapshot().phase).toBe("unknown");
  expect(client.canSend(chat)).toBe(false);
  expect(chat.execution.getSnapshot().canAcknowledge).toBe(false);
  await client.acknowledgeUnknown(chat);
  expect(acknowledgements()).toBe(0);
  expect(chat.execution.getSnapshot().phase).toBe("unknown");
  await Bun.sleep(20);
  expect(submissions).toHaveLength(1);
});

test("a typed refusal from an old session cannot unlock uncertain admission", async () => {
  const gate = Promise.withResolvers<undefined>();
  const { client, submissions } = await setup(undefined, false, {
    refuse: "Not admitted",
    beforeAdmission: gate.promise,
  });
  const chat = await readyDraft(client);
  client.send(chat);
  await Bun.sleep(10);
  expect(submissions).toHaveLength(1);
  client.disconnect("fixture-node");
  gate.resolve(undefined);
  await Bun.sleep(10);
  expect(chat.node).toBe("fixture-node");
  expect(chat.started).toBe(false);
  expect(chat.execution.getSnapshot().phase).toBe("unknown");
  expect(chat.draft.getSnapshot().text).toBe("Keep this draft");
});

test("native message admission refuses unsupported attachments before dispatch", async () => {
  const { client, submissions } = await setup();
  const chat = await readyDraft(client);
  chat.draft.update((value) => ({ ...value, attachments: ["notes.txt"] }));
  await Bun.sleep(5);
  expect(client.canSend(chat)).toBe(false);
  expect(client.submissionIssue(chat)).toContain("Attachments are not supported");
  client.send(chat);
  await Bun.sleep(5);
  expect(submissions).toHaveLength(0);
  expect(chat.draft.getSnapshot().attachments).toEqual(["notes.txt"]);
});

test.each(["a", "🙂", "\0"])(
  "native message admission counts scalars independently of UTF-8 and JSON expansion (%#)",
  async (character) => {
    const { client, submissions } = await setup();
    const chat = await readyDraft(client);
    const accepted = character.repeat(1_048_576);
    chat.draft.update((value) => ({ ...value, text: accepted }));
    await Bun.sleep(5);
    expect(client.submissionIssue(chat)).toBeNull();
    expect(client.canSend(chat)).toBe(true);
    const refused = `${accepted}a`;
    chat.draft.update((value) => ({ ...value, text: refused }));
    await Bun.sleep(5);
    expect(client.canSend(chat)).toBe(false);
    expect(client.submissionIssue(chat)).toContain("1048576 Unicode characters");
    client.send(chat);
    await Bun.sleep(5);
    expect(chat.draft.getSnapshot().text).toBe(refused);
    expect(submissions).toHaveLength(0);
  },
);

test.each(["unpaired \uD800", "unpaired \uDFFF"])(
  "native message admission refuses an unpaired surrogate without replacing the draft (%#)",
  async (text) => {
    const { client, submissions } = await setup();
    const chat = await readyDraft(client);
    chat.draft.update((value) => ({ ...value, text }));
    await Bun.sleep(5);
    expect(client.canSend(chat)).toBe(false);
    expect(client.submissionIssue(chat)).toContain("invalid Unicode text");
    client.send(chat);
    await Bun.sleep(5);
    expect(chat.draft.getSnapshot().text).toBe(text);
    expect(submissions).toHaveLength(0);
  },
);

test("a known failed run remains failed and allows a new user submission", async () => {
  const { client, finish } = await setup(undefined, false, { finishPhase: "failed" });
  const chat = await readyDraft(client);
  finish.resolve(undefined);
  client.send(chat);
  await Bun.sleep(20);
  expect(chat.execution.getSnapshot().phase).toBe("failed");
  chat.draft.update((value) => ({ ...value, text: "A new request" }));
  await Bun.sleep(5);
  expect(client.canSend(chat)).toBe(true);
});

test("uncertain tools require explicit acknowledgement and are never replayed by the browser", async () => {
  const { client, finish, submissions, acknowledgements } = await setup(undefined, false, {
    finishPhase: "unknown",
  });
  const chat = await readyDraft(client);
  finish.resolve(undefined);
  client.send(chat);
  await Bun.sleep(20);
  expect(chat.execution.getSnapshot().phase).toBe("unknown");
  expect(acknowledgements()).toBe(0);
  chat.draft.update((value) => ({ ...value, text: "Do something else" }));
  await Bun.sleep(5);
  expect(client.canSend(chat)).toBe(false);
  await client.acknowledgeUnknown(chat);
  expect(acknowledgements()).toBe(1);
  expect(submissions).toHaveLength(1);
  expect(chat.draft.getSnapshot().text).toBe("Do something else");
  expect(chat.execution.getSnapshot().phase).toBe("failed");
  expect(client.canSend(chat)).toBe(true);
});

test("bounded node projections retain explicit omission and truncation metadata", async () => {
  const ownerConversationId = "b682b418-4862-5ef7-a5a1-3c0388092824";
  const { client, finish } = await setup(undefined, false, { truncate: true, ownerConversationId });
  const chat = await readyDraft(client);
  finish.resolve(undefined);
  client.send(chat);
  await Bun.sleep(20);
  expect(chat.execution.getSnapshot().omittedMessages).toBe(1000);
  expect(chat.execution.getSnapshot().ownerConversationId).toBe(ownerConversationId);
  expect(chat.messages.getSnapshot().at(-1)).toMatchObject({ text: "Done", truncated: true });
});

test("a conversation catalog loads every bounded page before marking the node ready", async () => {
  const catalog = Array.from({ length: 340 }, (_, index) => ({
    id: `stored-${index}`,
    title: `Stored ${index}`,
    account: account.id,
    model: "fixture-model",
    effort: "low",
    archived: false,
    pinned: false,
  }));
  const { client, pages } = await setup(undefined, false, { catalog });
  expect(pages).toEqual([null, "32", "64", "96", "128", "160", "192", "224", "256", "288", "320"]);
  expect(client.roster.getSnapshot().filter((chat) => chat.started)).toHaveLength(340);
  const chat = await readyDraft(client);
  expect(chat.draft.getSnapshot().text).toBe("Keep this draft");
});

test("provider pages merge repeated account chunks into the full model catalog", async () => {
  const models = Array.from({ length: 700 }, (_, index) => ({
    id: `model-${index}`,
    label: `Model ${index}`,
    efforts: ["low"],
  }));
  const providerPages = [models.slice(0, 300), models.slice(300, 600), models.slice(600)].map(
    (chunk, index) => [
      { ...account, models: chunk, modelsTruncated: index < 2, available: index > 0 },
    ],
  );
  const { client, providerCursors } = await setup(undefined, false, { providerPages });
  expect(providerCursors).toEqual([null, "1", "2"]);
  expect(client.providers.state.getSnapshot().ready).toBe(true);
  expect(client.providers.state.getSnapshot().accounts[0]?.models).toEqual(models);
  expect(client.providers.state.getSnapshot().accounts[0]?.available).toBe(true);
});

test("terminal OAuth sessions notify subscribers when their node disconnects", async () => {
  const { client } = await setup(undefined, true);
  const session = await client.providers.authorization?.begin(
    { providerId: "openai", method: "oauth-browser" },
    new AbortController().signal,
  );
  if (session === undefined || session.retry === undefined) {
    throw new Error("Expected authorization.");
  }
  await session.complete("http://localhost:1455/auth/callback?code=fixture");
  let notifications = 0;
  const detach = session.subscribe(() => {
    notifications += 1;
  });
  client.disconnect("fixture-node");
  await Bun.sleep(5);
  expect(notifications).toBeGreaterThan(0);
  expect(session.getSnapshot()).toMatchObject({
    kind: "failed",
    error: "The node session changed. Begin a new sign-in.",
  });
  let retryFailure: unknown;
  try {
    await session.retry();
  } catch (error: unknown) {
    retryFailure = error;
  }
  expect(retryFailure).toBeInstanceOf(Error);
  detach();
  session.dispose();
});

test("cancelling a connected OAuth session does not replace the confirmed account", async () => {
  const { client } = await setup();
  const session = await client.providers.authorization?.begin(
    { providerId: "openai", method: "oauth-browser" },
    new AbortController().signal,
  );
  if (session === undefined) {
    throw new Error("Expected authorization.");
  }
  await session.complete("http://localhost:1455/auth/callback?code=fixture");
  await session.cancel();
  expect(session.getSnapshot()).toMatchObject({ kind: "connected", account });
  session.dispose();
});

test("reconnecting an OAuth account passes its existing identity to the native service", async () => {
  const { client, authorizationKeys } = await setup();
  const session = await client.providers.authorization?.begin(
    { providerId: "openai", method: "oauth-browser", accountId: account.id },
    new AbortController().signal,
  );
  expect(authorizationKeys).toHaveLength(1);
  expect(authorizationKeys[0]?.[0]).toBe("openai");
  expect(authorizationKeys[0]?.[2]).toBe(account.id);
  session?.dispose();
});

test("a bounded OAuth account preview cannot replace a full provider catalog", async () => {
  const models = Array.from({ length: 40 }, (_, index) => ({
    id: `model-${index}`,
    label: `Model ${index}`,
    efforts: ["low"],
  }));
  const full = { ...account, models, authorizationMethod: "oauth-browser" };
  const { client } = await setup(undefined, false, {
    providerPages: [[full]],
    completeAccount: { ...full, models: models.slice(0, 1), modelsTruncated: true },
  });
  const session = await client.providers.authorization?.begin(
    { providerId: "openai", method: "oauth-browser" },
    new AbortController().signal,
  );
  if (session === undefined) {
    throw new Error("Expected authorization.");
  }
  await session.complete("http://localhost:1455/auth/callback?code=fixture");
  await Bun.sleep(10);
  expect(client.providers.state.getSnapshot().accounts[0]?.models).toEqual(models);
  expect(client.providers.state.getSnapshot().accounts[0]?.authorizationMethod).toBe(
    "oauth-browser",
  );
  session.dispose();
});

test("node catalog restores the versioned checkout binding and refuses another node's context", async () => {
  const context: ConversationExecutionContext = {
    version: 1,
    nodeId: "fixture-node",
    principal: "fixture-user",
    path: "/project/worktree",
    owner: "git",
    identity: JSON.stringify([
      "/project/worktree",
      "/project/.git/worktrees/worktree",
      "/project/.git",
    ]),
  };
  const { client } = await setup(undefined, false, {
    catalog: [
      {
        id: "checkout-chat",
        title: "Checkout chat",
        account: "fixture-account",
        model: "fixture-model",
        effort: "low",
        archived: false,
        pinned: true,
        executionContext: context,
      },
    ],
  });
  const restored = client.roster.getSnapshot().find((chat) => chat.id === "checkout-chat");
  expect(restored?.executionContext).toEqual(context);
  expect(restored?.draft.getSnapshot().executionContext).toEqual(context);
  if (restored === undefined) {
    throw new Error("Expected restored checkout conversation.");
  }
  expect(() => {
    client.setConversationContext(restored, { ...context, path: "/other" });
  }).toThrow("already bound");
});

test("an idle archive exposes retention failure separately from confirmed chat metadata", async () => {
  const { client } = await setup(undefined, false, {
    lifecycleWarning: "Archive capture was denied; the checkout remains retained.",
    catalog: [
      {
        id: "idle-archive",
        title: "Idle chat",
        account: "fixture-account",
        model: "fixture-model",
        effort: "low",
        archived: false,
        pinned: false,
      },
    ],
  });
  const chat = client.roster.getSnapshot().find((item) => item.id === "idle-archive");
  if (chat === undefined) {
    throw new Error("Expected restored conversation.");
  }
  client.archive(chat);
  await Bun.sleep(30);
  expect(chat.archived).toBe(true);
  expect(chat.execution.getSnapshot().lifecycleWarning).toContain("Archive capture was denied");
});
