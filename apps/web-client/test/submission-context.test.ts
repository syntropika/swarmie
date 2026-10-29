import { expect, test } from "bun:test";

import { createConversationRecords } from "@swarmie/plugin-conversations-browser";
import type { ConversationExecutionContext } from "@swarmie/plugin-conversations-browser";
import { createHost } from "@syntropika/tembra-sdk/node";
import { sleep } from "bun";

import { initializeClient } from "../src/client/initialize";
import { createNodeExecution } from "../src/client/node-execution";
import { createSubmissions } from "../src/client/submissions";
import { fixtureConnector, fixtureNode } from "./connection-fixture";

test("execution context is captured before admission and cannot change after uncertain dispatch", async () => {
  const client = await initializeClient(createHost(), {
    dev: false,
    nodes: [fixtureNode],
    connector: fixtureConnector,
  });
  const admission = Promise.withResolvers<undefined>();
  const approval = Promise.withResolvers<ConversationExecutionContext | undefined>();
  let submitted: ConversationExecutionContext | undefined;
  let calls = 0;
  const submissions = createSubmissions(
    client.nodeConnections,
    client.connectionState,
    createConversationRecords(client.conversationState, client.conversationState.commit),
    client.providers,
    () => ({
      submit: async (input) => {
        calls += 1;
        submitted = input.executionContext;
        throw new Error("The response was lost.");
      },
      stop: async () => undefined,
    }),
    client.conversationsUi.lifecycle,
    client.nextId,
    null,
    async ({ context }) => {
      admission.resolve(undefined);
      await approval.promise;
      return context;
    },
  );
  try {
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
      text: "Review changes",
      account: "account",
      model: "model",
    }));
    const context = {
      nodeId: fixtureNode.id,
      principal: fixtureNode.principal,
      path: "/project/worktree",
      owner: "git",
    };
    submissions.setConversationContext(chat, context);
    context.path = "/another";
    await sleep(10);
    submissions.send(chat);
    await admission.promise;
    expect(calls).toBe(0);
    expect(() => {
      submissions.setConversationContext(chat, { nodeId: fixtureNode.id, path: "/other" });
    }).toThrow("already bound");
    approval.resolve(undefined);
    await sleep(25);
    expect(submitted).toEqual({
      nodeId: fixtureNode.id,
      principal: fixtureNode.principal,
      path: "/project/worktree",
      owner: "git",
    });
    expect(chat.executionContext).toEqual(submitted);
    expect(chat.execution.getSnapshot().phase).toBe("unknown");
    submissions.send(chat);
    await sleep(10);
    expect(calls).toBe(1);
  } finally {
    await client.close();
  }
});

test("failed preparation prevents admission and leaves an editable draft", async () => {
  const client = await initializeClient(createHost(), {
    dev: false,
    nodes: [fixtureNode],
    connector: fixtureConnector,
  });
  let calls = 0;
  const submissions = createSubmissions(
    client.nodeConnections,
    client.connectionState,
    createConversationRecords(client.conversationState, client.conversationState.commit),
    client.providers,
    () => ({
      submit: async () => {
        calls += 1;
        return { accepted: true };
      },
      stop: async () => undefined,
    }),
    client.conversationsUi.lifecycle,
    client.nextId,
    null,
    async () => {
      throw new Error("Setup failed. Retry preparation explicitly.");
    },
  );
  try {
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
      text: "Keep this draft",
      account: "account",
      model: "model",
    }));
    submissions.setConversationContext(chat, { nodeId: fixtureNode.id, path: "/project" });
    await sleep(10);
    submissions.send(chat);
    await sleep(25);
    expect(calls).toBe(0);
    expect(chat.node).toBeNull();
    expect(chat.executionContext).toBeUndefined();
    expect(chat.draft.getSnapshot().text).toBe("Keep this draft");
    expect(chat.execution.getSnapshot().phase).toBe("idle");
    expect(client.conversationError.getSnapshot()).toContain("Setup failed");
  } finally {
    await client.close();
  }
});

test("native conversations refuse an unsupported checkout before acquiring an execution service", async () => {
  const client = await initializeClient(createHost(), {
    dev: false,
    nodes: [fixtureNode],
    connector: fixtureConnector,
  });
  const execution = createNodeExecution(
    client.connectionState,
    client.nodeConnections,
    createConversationRecords(client.conversationState, client.conversationState.commit),
    () => client.activeConversation().id,
  );
  try {
    const connection = await client.nodeConnections.open(fixtureNode.id);
    const context = { nodeId: fixtureNode.id, path: "/project/worktree", owner: "git" };
    const input = {
      conversationId: "fixture-conversation",
      submissionId: "fixture-submission",
      createConversation: true,
      draft: client.activeConversation().draft.getSnapshot(),
      mode: "queue" as const,
    };
    const api = execution.factory(connection);
    const outer = await api.submit(
      { ...input, executionContext: context },
      new AbortController().signal,
    );
    const draft = await api.submit(
      { ...input, draft: { ...input.draft, executionContext: context } },
      new AbortController().signal,
    );
    for (const result of [outer, draft]) {
      expect(result.accepted).toBe(false);
      expect("reason" in result ? result.reason : "").toContain("captured checkout");
    }
  } finally {
    execution.dispose();
    await client.close();
  }
});
