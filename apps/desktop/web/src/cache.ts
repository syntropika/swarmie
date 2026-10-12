import { clientRequest, nativeClient } from "./clientTransport";
import type { Project } from "./protocol";
import type { Thread } from "./protocol";
import type { LocalStoreIdentity } from "@swarmie/contracts";
import { nodeKey, retainNode, sameNode, validNodeIdentity } from "./nodeScope";
export type CachedWorkspace = { threads: Thread[]; revision: number; metadata?: { projects: Project[]; activeId: string; customChatDirectory?: string } };
export class WorkspaceCache {
  private database?: Promise<IDBDatabase>;
  private readonly node: LocalStoreIdentity;
  constructor(node: LocalStoreIdentity) {
    this.node = retainNode(node);
  }
  available() {
    return nativeClient() || typeof indexedDB !== "undefined";
  }
  private open(): Promise<IDBDatabase> {
    if (!this.available())
      return Promise.reject(
        new Error(
          "This WebView does not support persistent attachment storage.",
        ),
      );
    return (this.database ??= new Promise((resolve, reject) => {
      const request = indexedDB.open("swarmie.desktop.cache", 1);
      request.onupgradeneeded = () => {
        request.result.createObjectStore("workspace");
      };
      request.onsuccess = () => resolve(request.result);
      request.onerror = () =>
        reject(request.error ?? new Error("Cannot open attachment storage."));
      request.onblocked = () =>
        reject(
          new Error("Close other swarmie windows to update attachment storage."),
        );
    }));
  }
  async load(): Promise<CachedWorkspace | undefined> {
    if (!this.available()) return undefined;
    if (nativeClient()) {
      const saved = await this.native<CachedWorkspace & { node?: LocalStoreIdentity }>("client/cacheLoad", {});
      return saved && validNodeIdentity(saved.node) && sameNode(saved.node, this.node) && Array.isArray(saved.threads) ? saved : undefined;
    }
    const database = await this.open();
    return new Promise((resolve, reject) => {
      const transaction = database.transaction("workspace", "readonly");
      const request = transaction.objectStore("workspace").get(nodeKey(this.node));
      request.onsuccess = () => {
        const saved = request.result as (CachedWorkspace & { node?: LocalStoreIdentity }) | undefined;
        resolve(saved && validNodeIdentity(saved.node) && sameNode(saved.node, this.node) && Array.isArray(saved.threads)
          ? saved : undefined);
      };
      request.onerror = () => reject(request.error);
    });
  }
  private async native<T>(method: string, params: unknown): Promise<T> {
    const response = await clientRequest("/client/cache", { jsonrpc: "2.0", id: crypto.randomUUID(), method, params: { node: this.node, ...(params as object) } }, "", 30_000);
    const value = await response.json();
    if (value.error) throw new Error(value.error.message);
    return value.result as T;
  }
  async save(value: CachedWorkspace): Promise<void> {
    if (nativeClient()) { await this.native("client/cacheSave", { value: { ...value, node: this.node } }); return; }
    const database = await this.open();
    await new Promise<void>((resolve, reject) => {
      const transaction = database.transaction("workspace", "readwrite");
      transaction.objectStore("workspace").put({ ...value, node: this.node }, nodeKey(this.node));
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
