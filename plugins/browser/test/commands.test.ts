import { expect, test } from "bun:test";

import { bindState } from "@swarmie/client-ui";
import { createHost } from "@syntropika/tembra-sdk/node";

import { createViewsApi, viewsOrb } from "../src/views";

test("capability tabs capture targets and separate nodes and checkouts", async () => {
  const host = createHost();
  await host.install(viewsOrb);
  const owner = await host.get(viewsOrb.services["swarmie.views.ui"], "main");
  const failures: unknown[] = [];
  function failed(error: unknown) {
    failures.push(error);
  }
  const state = bindState(owner, failed);
  let id = 0;
  const api = createViewsApi(state, () => {
    id += 1;
    return String(id);
  });
  const flush = async () => {
    await owner.update((current) => current);
  };
  try {
    api.open("start", { id: "conversation", title: "Conversation" });
    await flush();
    const startingId = state.getSnapshot().tab;
    const target = {
      nodeId: "one",
      principal: "user",
      path: "/project",
      detail: { mode: "changes", index: 2 },
    };
    api.openCapability({ owner: "git", view: "repository", title: "Project", target });
    await flush();
    const [first] = state.getSnapshot().tabs;
    expect(state.getSnapshot().tabs).toHaveLength(1);
    expect(first?.id).toBe(startingId);
    target.path = "/changed";
    expect(first?.kind).toBe("capability");
    expect(first && "target" in first ? first.target.path : null).toBe("/project");
    api.openCapability({
      owner: "git",
      view: "repository",
      title: "Project",
      target: {
        path: "/project",
        principal: "user",
        nodeId: "one",
        detail: { index: 2, mode: "changes" },
      },
    });
    await flush();
    expect(state.getSnapshot().tabs).toHaveLength(1);
    api.openCapability({
      owner: "git",
      view: "repository",
      title: "Project",
      target: { nodeId: "two", principal: "user", path: "/project" },
    });
    api.openCapability({
      owner: "git",
      view: "repository",
      title: "Worktree",
      target: { nodeId: "one", principal: "user", path: "/worktree" },
    });
    await flush();
    expect(state.getSnapshot().tabs).toHaveLength(3);
    expect(failures).toEqual([]);
  } finally {
    owner.release();
    await host.close();
  }
});

test("fullscreen retains the active capability tab and restores the conversation", async () => {
  const host = createHost();
  await host.install(viewsOrb);
  const owner = await host.get(viewsOrb.services["swarmie.views.ui"], "main");
  const failures: unknown[] = [];
  function failed(error: unknown) {
    failures.push(error);
  }
  const state = bindState(owner, failed);
  let id = 0;
  const api = createViewsApi(state, () => {
    id += 1;
    return String(id);
  });
  const flush = async () => {
    await owner.update((current) => current);
  };
  try {
    api.openCapability({
      owner: "git",
      view: "repository",
      title: "Changes",
      target: { nodeId: "one", path: "/project" },
    });
    await flush();
    const selected = state.getSnapshot().tab;
    api.expand({ id: "chat", title: "Conversation" });
    await flush();
    expect(state.getSnapshot().expanded).toBe(true);
    expect(state.getSnapshot().conversationVisible).toBe(false);
    expect(state.getSnapshot().tab).toBe(selected);
    expect(state.getSnapshot().tabs.filter((tab) => tab.kind === "conversation")).toHaveLength(0);
    api.expand({ id: "chat", title: "Conversation" });
    await flush();
    expect(state.getSnapshot().tab).toBe(selected);
    expect(state.getSnapshot().conversationVisible).toBe(true);
    expect(state.getSnapshot().tabs).toHaveLength(1);
  } finally {
    owner.release();
    await host.close();
  }
});
