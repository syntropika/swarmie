import { expect, test } from "bun:test";

import { createConversationRecords } from "@swarmie/plugin-conversations-browser";
import type {
  ConversationExecutionApi,
  ConversationExecutionContext,
} from "@swarmie/plugin-conversations-browser";
import { createHost } from "@syntropika/tembra-sdk/node";
import { sleep } from "bun";

import { initializeClient } from "../src/client/initialize";
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

test("first checkout admission finalizes both request and draft contexts independently of property order", async () => {
  const client = await initializeClient(createHost(), {
    dev: false,
    nodes: [fixtureNode],
    connector: fixtureConnector,
  });
  const records = createConversationRecords(
    client.conversationState,
    client.conversationState.commit,
  );
  const submitted = Promise.withResolvers<Parameters<ConversationExecutionApi["submit"]>[0]>();
  const context = {
    nodeId: fixtureNode.id,
    principal: fixtureNode.principal,
    path: "/selected/alias",
    owner: "git",
  };
  const finalized = {
    ...context,
    version: 1 as const,
    path: "/project",
    identity: JSON.stringify(["/project", "/project/.git", "/project/.git"]),
  };
  const submissions = createSubmissions(
    client.nodeConnections,
    client.connectionState,
    records,
    client.providers,
    () => ({
      submit: async (input) => {
        submitted.resolve(input);
        return { accepted: true };
      },
      stop: async () => undefined,
    }),
    client.conversationsUi.lifecycle,
    client.nextId,
    null,
    async () => finalized,
  );
  try {
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
    const chat = client.activeConversation();
    submissions.setConversationContext(chat, context);
    chat.draft.update((draft) => ({
      ...draft,
      text: "Review this checkout",
      account: "account",
      model: "model",
    }));
    await sleep(10);
    submissions.send(chat);
    const input = await submitted.promise;
    expect(input.executionContext).toEqual(finalized);
    expect(input.draft.executionContext).toEqual(finalized);
    expect(input.draft.executionContext?.version).toBe(1);
    expect(input.draft.executionContext?.identity).toBe(finalized.identity);
    expect(context.path).toBe("/selected/alias");
    const { sameExecutionContext } = await import("../src/client/conversation-values");
    expect(
      sameExecutionContext(finalized, {
        identity: finalized.identity,
        owner: "git",
        path: "/project",
        principal: fixtureNode.principal,
        nodeId: fixtureNode.id,
        version: 1,
      }),
    ).toBe(true);
  } finally {
    await client.close();
  }
});
