import "fake-indexeddb/auto";
import { expect, it, vi } from "vitest";
import { WorkspaceCache, hydrateThreads } from "./cache";
import { newThread } from "./protocol";
import { WorkspaceStore } from "./workspace";
import { workspaceStorageKey } from "./nodeScope";
const node = { environmentId: "cache-node", storeId: "cache-store", ownerScopeId: "cache-owner" };
it("roundtrips an 8 MiB image draft through IndexedDB and lightweight metadata", async () => {
  const local = new Map<string, string>();
  vi.stubGlobal("localStorage", {
    getItem: (key: string) => local.get(key) ?? null,
    setItem: (key: string, value: string) => {
      if (value.length > 5 * 1024 * 1024)
        throw new Error("localStorage quota exceeded");
      local.set(key, value);
    },
  });
  const first = new WorkspaceStore({ node });
  first.createThread();
  const id = first.active()!.id;
  const data = "A".repeat(Math.ceil((8 * 1024 * 1024) / 3) * 4);
  first.updateThread(id, {
    draft: "Inspect attached image",
    attachments: [
      { id: "image", name: "large.png", mime: "image/png", data, image: true },
    ],
    fileDrafts: {
      "draft.rs": { content: "Unsaved edit", original: "Original file" },
    },
  });
  await first.flushPersistence();
  const stored = local.get(workspaceStorageKey(node))!;
  expect(stored.length).toBeLessThan(10000);
  expect(stored).not.toContain(data.slice(0, 1024));
  const second = new WorkspaceStore({ node });
  await second.flushPersistence();
  expect(second.active()!.draft).toBe("Inspect attached image");
  expect(second.active()!.attachments![0].data).toBe(data);
  expect(second.active()!.fileDrafts?.["draft.rs"]).toEqual({
    content: "Unsaved edit",
    original: "Original file",
  });
  expect(second.getSnapshot().error).toBe("");
});
it("late cache hydration cannot overwrite a newer local draft and attachment edit", () => {
  const current = {
    ...newThread("p", "w", "pi"),
    id: "thread",
    draft: "Newer",
    attachments: [],
  };
  const cached = {
    ...current,
    draft: "Older",
    attachments: [
      {
        id: "old",
        name: "old.txt",
        mime: "text/plain",
        data: "old",
        image: false,
      },
    ],
  };
  expect(
    hydrateThreads(
      [current],
      [cached],
      new Map([["thread", new Set(["draft", "attachments"] as const)]]),
    )[0],
  ).toEqual(current);
});
it("restores dirty file drafts despite early layout changes during hydration", () => {
  const cached = {
    ...newThread("p", "w", "pi"),
    fileDrafts: {
      "hello.txt": { content: "Unsaved native editor", original: "First line" },
    },
  };
  const current = {
    ...cached,
    fileDrafts: undefined,
    layout: { panel: "extensions" },
  };
  const hydrated = hydrateThreads(
    [current],
    [cached],
    new Map([[cached.id, new Set(["layout" as const])]]),
  )[0];
  expect(hydrated.fileDrafts).toEqual(cached.fileDrafts);
  expect(hydrated.layout).toEqual(current.layout);
});
it("reports failed persistent attachment writes visibly", async () => {
  vi.stubGlobal("localStorage", { getItem: () => null, setItem: vi.fn() });
  const save = vi
    .spyOn(WorkspaceCache.prototype, "save")
    .mockRejectedValueOnce(new Error("Disk quota"));
  const workspace = new WorkspaceStore({ node });
  workspace.createThread();
  workspace.updateThread(workspace.active()!.id, {
    draft: "Keep this",
    attachments: [
      {
        id: "x",
        name: "x.txt",
        mime: "text/plain",
        data: "content",
        image: false,
      },
    ],
  });
  await workspace.flushPersistence();
  expect(workspace.getSnapshot().error).toContain("Draft storage failed");
  expect(workspace.active()!.attachments).toHaveLength(1);
  save.mockRestore();
});

it("restores native drafts and view metadata after the WebView origin storage is empty", async () => {
  let nativeSaved: unknown = {
    node, revision: 1,
    threads: [{ ...newThread("project", "/work", "mock"), id: "native-draft", draft: "Keep this draft", attachments: [{ id: "file", name: "note.txt", mime: "text/plain", data: "hello", image: false }] }],
    metadata: { projects: [], activeId: "native-draft", customChatDirectory: "/work" },
  };
  vi.stubGlobal("localStorage", { getItem: () => null, setItem: vi.fn() });
  vi.stubGlobal("window", { location: { search: "?client=native" }, addEventListener: vi.fn(), zero: { invoke: async (_command: string, payload: any) => {
    expect(payload.path).toBe("/client/cache");
    const request = payload.request;
    if (request.method === "client/cacheSave") nativeSaved = request.params.value;
    return { response: { jsonrpc: "2.0", id: request.id, result: request.method === "client/cacheLoad" ? nativeSaved : null } };
  } } });
  try {
    const store = new WorkspaceStore({ node });
    await store.flushPersistence();
    expect(store.active()?.id).toBe("native-draft");
    expect(store.active()?.draft).toBe("Keep this draft");
    expect(store.active()?.attachments?.[0].data).toBe("hello");
    expect(store.getSnapshot().customChatDirectory).toBe("/work");
  } finally { vi.unstubAllGlobals(); }
});
