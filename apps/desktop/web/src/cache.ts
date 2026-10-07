import type { Thread } from "./protocol";
export type CachedWorkspace = { threads: Thread[]; revision: number };
export class WorkspaceCache {
  private database?: Promise<IDBDatabase>;
  available() {
    return typeof indexedDB !== "undefined";
  }
  private open(): Promise<IDBDatabase> {
    if (!this.available())
      return Promise.reject(
        new Error(
          "This WebView does not support persistent attachment storage.",
        ),
      );
    return (this.database ??= new Promise((resolve, reject) => {
      const request = indexedDB.open("mpai.desktop.cache", 1);
      request.onupgradeneeded = () => {
        request.result.createObjectStore("workspace");
      };
      request.onsuccess = () => resolve(request.result);
      request.onerror = () =>
        reject(request.error ?? new Error("Cannot open attachment storage."));
      request.onblocked = () =>
        reject(
          new Error("Close other mpai windows to update attachment storage."),
        );
    }));
  }
  async load(): Promise<CachedWorkspace | undefined> {
    if (!this.available()) return undefined;
    const database = await this.open();
    return new Promise((resolve, reject) => {
      const transaction = database.transaction("workspace", "readonly");
      const request = transaction.objectStore("workspace").get("current");
      request.onsuccess = () =>
        resolve(request.result as CachedWorkspace | undefined);
      request.onerror = () => reject(request.error);
    });
  }
  async save(value: CachedWorkspace): Promise<void> {
    const database = await this.open();
    await new Promise<void>((resolve, reject) => {
      const transaction = database.transaction("workspace", "readwrite");
      transaction.objectStore("workspace").put(value, "current");
      transaction.oncomplete = () => resolve();
      transaction.onerror = () =>
        reject(
          transaction.error ?? new Error("Cannot save attachment drafts."),
        );
      transaction.onabort = () =>
        reject(
          transaction.error ??
            new Error("Attachment storage transaction was cancelled."),
        );
    });
  }
}
export function hydrateThreads(
  current: Thread[],
  cached: Thread[],
  edited: ReadonlyMap<string, ReadonlySet<keyof Thread>>,
): Thread[] {
  return current.map((thread) => {
    const saved = cached.find((t) => t.id === thread.id);
    if (!saved) return thread;
    const hydrated = {
      ...saved,
      ...thread,
      attachments: saved.attachments,
      fileDrafts: saved.fileDrafts,
      promptRecords: saved.promptRecords,
      messages: saved.messages,
      tools: saved.tools,
      permissions: saved.permissions,
      reasoning: saved.reasoning,
      plan: saved.plan,
      subagents: saved.subagents,
      usage: saved.usage,
      context: saved.context,
      queue: saved.queue,
    };
    for (const key of edited.get(thread.id) ?? [])
      Object.defineProperty(hydrated, key, {
        value: thread[key],
        enumerable: true,
        configurable: true,
        writable: true,
      });
    return hydrated;
  });
}
