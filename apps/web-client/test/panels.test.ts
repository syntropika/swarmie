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
