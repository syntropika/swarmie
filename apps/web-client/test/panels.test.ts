import { expect, test } from "bun:test";

import { createHost } from "@syntropika/tembra-sdk/node";
import { sleep } from "bun";

import { initializeClient } from "../src/client/initialize";

test("showing the conversation from expanded Views restores one dedicated conversation", async () => {
  const client = await initializeClient(createHost(), { dev: false });
  try {
    const chat = client.activeConversation();
    chat.draft.update((draft) => ({ ...draft, text: "Keep this draft" }));
    client.openTab();
    await sleep(10);
    const [tab] = client.shell.getSnapshot().tabs;
    if (!tab) {
      throw new Error("Expected a view tab.");
    }
    client.expandView();
    await sleep(10);
    expect(client.shell.getSnapshot().tabs.some((item) => item.kind === "conversation")).toBe(true);

    client.toggleConversation();
    await sleep(10);
    const layout = client.shell.getSnapshot();
    expect(layout.expanded).toBe(false);
    expect(layout.conversationVisible).toBe(true);
    expect(layout.tabs).toEqual([tab]);
    expect(layout.tab).toBe(tab.id);
    expect(client.activeConversation().id).toBe(chat.id);
    expect(chat.draft.getSnapshot().text).toBe("Keep this draft");
  } finally {
    await client.close();
  }
});

test("the mobile conversation switch restores a conversation tab without duplicating it", async () => {
  const client = await initializeClient(createHost(), { dev: false });
  try {
    client.openTab();
    await sleep(10);
    client.expandView();
    await sleep(10);
    const tab = client.shell.getSnapshot().tabs.find((item) => item.kind === "conversation");
    if (!tab) {
      throw new Error("Expected the expanded conversation tab.");
    }
    client.views.update((current) => ({ ...current, tab: tab.id }));
    await sleep(10);
    client.showConversation();
    await sleep(10);
    const layout = client.shell.getSnapshot();
    expect(layout.mobile).toBe("conversation");
    expect(layout.expanded).toBe(false);
    expect(layout.conversationVisible).toBe(true);
    expect(layout.tabs).toHaveLength(1);
    expect(layout.tabs[0]?.kind).toBe("start");
    expect(layout.tab).toBe(layout.tabs[0]?.id ?? "");
  } finally {
    await client.close();
  }
});

test("hiding the conversation keeps the view and closing the last view restores the conversation", async () => {
  const client = await initializeClient(createHost(), { dev: false });
  try {
    client.openTab();
    await sleep(10);
    const [tab] = client.shell.getSnapshot().tabs;
    if (!tab) {
      throw new Error("Expected a view tab.");
    }
    client.toggleConversation();
    await sleep(10);
    expect(client.shell.getSnapshot().conversationVisible).toBe(false);
    expect(client.shell.getSnapshot().tabs).toEqual([tab]);
    client.closeTab(tab.id);
    await sleep(10);
    expect(client.shell.getSnapshot().tabs).toHaveLength(0);
    expect(client.shell.getSnapshot().conversationVisible).toBe(true);
    expect(client.shell.getSnapshot().expanded).toBe(false);
  } finally {
    await client.close();
  }
});

test("a side chat keeps the original record in Views and starts an independent draft", async () => {
  const client = await initializeClient(createHost(), { dev: false });
  try {
    const original = client.activeConversation();
    client.conversationState.update((state) => ({
      ...state,
      records: state.records.map((record) =>
        record.id === original.id ? { ...record, node: "bound-node", started: true } : record,
      ),
    }));
    original.draft.update((draft) => ({
      ...draft,
      text: "Keep my draft",
      attachments: ["notes.txt"],
    }));
    original.messages.update(() => [{ id: "message", role: "user", text: "Existing message" }]);
    original.execution.update((execution) => ({
      ...execution,
      phase: "unknown",
      label: "Reconcile first",
    }));
    await sleep(10);
    const draft = original.draft.getSnapshot();
    const messages = original.messages.getSnapshot();
    const execution = original.execution.getSnapshot();
    const fresh = client.newSideChat(original);
    await sleep(10);
    const layout = client.shell.getSnapshot();
    expect(fresh.id).not.toBe(original.id);
    expect(client.activeConversation().id).toBe(fresh.id);
    expect(fresh.draft.getSnapshot().text).toBe("");
    expect(fresh.draft.getSnapshot().attachments).toEqual([]);
    expect(fresh.messages.getSnapshot()).toEqual([]);
    expect(fresh.execution.getSnapshot().phase).toBe("idle");
    expect(original.draft.getSnapshot()).toEqual(draft);
    expect(original.messages.getSnapshot()).toEqual(messages);
    expect(original.execution.getSnapshot()).toEqual(execution);
    expect(original.node).toBe("bound-node");
    expect(original.started).toBe(true);
    expect(layout.tabs).toHaveLength(1);
    expect(layout.tabs[0]).toMatchObject({ kind: "conversation", conversationId: original.id });
    expect(layout.tab).toBe(layout.tabs[0]?.id ?? "");
    expect(layout.expanded).toBe(false);
    expect(layout.conversationVisible).toBe(true);
    expect(layout.mobile).toBe("conversation");
    fresh.draft.update((value) => ({ ...value, text: "Independent" }));
    await sleep(10);
    expect(original.draft.getSnapshot()).toEqual(draft);
    expect(client.roster.getSnapshot().find((chat) => chat.id === original.id)).toBe(original);
  } finally {
    await client.close();
  }
});

test("side conversations survive expansion, restoration and closing another side tab", async () => {
  const client = await initializeClient(createHost(), { dev: false });
  try {
    const first = client.activeConversation();
    const second = client.newSideChat(first);
    await sleep(10);
    const third = client.newSideChat(second);
    await sleep(10);
    const sideTabs = client.shell.getSnapshot().tabs;
    expect(sideTabs).toHaveLength(2);
    expect(sideTabs.map((tab) => (tab.kind === "conversation" ? tab.conversationId : ""))).toEqual([
      first.id,
      second.id,
    ]);
    client.expandView();
    await sleep(10);
    expect(client.shell.getSnapshot().tabs).toHaveLength(3);
    client.showConversation();
    await sleep(10);
    expect(client.shell.getSnapshot().tabs).toEqual(sideTabs);
    const [firstTab] = sideTabs;
    if (!firstTab) {
      throw new Error("Expected the first side tab.");
    }
    client.closeTab(firstTab.id);
    await sleep(10);
    expect(client.shell.getSnapshot().tabs).toEqual(sideTabs.slice(1));
    expect(client.activeConversation().id).toBe(third.id);
    expect(client.roster.getSnapshot().some((chat) => chat.id === first.id)).toBe(true);
  } finally {
    await client.close();
  }
});

test("starting a side chat from expanded Views restores both panes", async () => {
  const client = await initializeClient(createHost(), { dev: false });
  try {
    client.openTab();
    await sleep(10);
    client.expandView();
    await sleep(10);
    const original = client.activeConversation();
    client.newSideChat(original);
    await sleep(10);
    const layout = client.shell.getSnapshot();
    expect(layout.expanded).toBe(false);
    expect(layout.conversationVisible).toBe(true);
    expect(layout.tabs.filter((tab) => tab.kind === "conversation")).toHaveLength(1);
    expect(layout.tabs.some((tab) => tab.kind === "conversation" && tab.temporary === true)).toBe(
      false,
    );
    expect(client.activeConversation().id).not.toBe(original.id);
  } finally {
    await client.close();
  }
});
