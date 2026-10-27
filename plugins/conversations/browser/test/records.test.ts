import { expect, test } from "bun:test";

import { bindState } from "@swarmie/client-ui";
import { createHost } from "@syntropika/tembra-sdk/node";

import { conversationsOrb, createConversationRecords } from "../src";
import type { ConversationData } from "../src";

const conversation: ConversationData = {
  id: "conversation",
  title: "New conversation",
  started: false,
  node: null,
  pinned: false,
  archived: false,
  draft: {
    text: "",
    attachments: [],
    model: "",
    account: null,
    effort: "",
    node: "node",
    permission: "Ask",
  },
  messages: [],
  execution: { phase: "idle", label: "", queue: [], steering: [] },
};

test("conversation records keep live facades and metadata snapshots on the owning State", async () => {
  const host = createHost();
  await host.install(conversationsOrb);
  const owner = await host.get(conversationsOrb.services["swarmie.conversations.ui"], "main");
  const failures: unknown[] = [];
  function failed(error: unknown) {
    failures.push(error);
  }
  try {
    const state = bindState(owner, failed);
    const records = createConversationRecords(state, owner.update);
    const view = records.create(conversation);
    await records.commit(view.id, (record) => record);
    const initial = records.roster.getSnapshot();
    expect(initial[0]).toBe(view);

    view.draft.update((draft) => ({ ...draft, text: "Keep this draft" }));
    await records.commit(view.id, (record) => ({
      ...record,
      messages: [{ id: "message", role: "user", text: "Hello" }],
    }));
    expect(records.roster.getSnapshot()).toBe(initial);
    expect(view.draft.getSnapshot().text).toBe("Keep this draft");
    expect(view.messages.getSnapshot()[0]?.text).toBe("Hello");

    records.update(view.id, (record) => ({ ...record, title: "Renamed", pinned: true }));
    await records.commit(view.id, (record) => record);
    expect(records.roster.getSnapshot()).not.toBe(initial);
    expect(records.roster.getSnapshot()[0]).toBe(view);
    expect(view.title).toBe("Renamed");
    expect(view.pinned).toBe(true);
    records.fail(new Error("Unavailable"));
    await records.commit(view.id, (record) => record);
    expect(state.getSnapshot().error).toBe("Unavailable");
    expect(failures).toEqual([]);
  } finally {
    owner.release();
    await host.close();
  }
});
