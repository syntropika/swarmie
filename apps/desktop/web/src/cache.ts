import type { LocalStoreIdentity } from "@swarmie/contracts";

import { responseResult } from "~/web/src/checked-json";
import { clientRequest, nativeClient } from "~/web/src/client-transport";
import { decodeCachedWorkspace } from "~/web/src/connections";
import { nodeKey, retainNode, sameNode, validNodeIdentity } from "~/web/src/node-scope";
import type { Project, Thread } from "~/web/src/protocol";

export type CachedWorkspace = {
  threads: Thread[];
  revision: number;
  metadata?: { projects: Project[]; activeId: string; customChatDirectory?: string };
};
export class WorkspaceCache {
  private database?: Promise<IDBDatabase>;
  private readonly node: LocalStoreIdentity;
  constructor(node: LocalStoreIdentity) {
    this.node = retainNode(node);
  }
  available = cacheAvailable;
  private async open(): Promise<IDBDatabase> {
    if (!this.available()) {
      throw new Error("This WebView does not support persistent attachment storage.");
    }
    if (this.database) {
      return await this.database;
    }
    // Native IDB callbacks settle once. A blocked open is rejected; a later success closes its unused connection.
    // oxlint-disable-next-line promise/avoid-new -- IndexedDB exposes callback events, and this adapter owns settlement and late connection cleanup.
    const pending = new Promise<IDBDatabase>((resolve, reject) => {
      const request = indexedDB.open("swarmie.desktop.cache", 1);
      let rejected = false;
      request.addEventListener("upgradeneeded", () => {
        request.result.createObjectStore("workspace");
      });
      request.addEventListener(
        "success",
        () => {
          if (rejected) {
            request.result.close();
            return;
          }
          resolve(request.result);
        },
        { once: true },
      );
      request.addEventListener(
        "error",
        () => {
          rejected = true;
          reject(request.error ?? new Error("Cannot open attachment storage."));
        },
        { once: true },
      );
      request.addEventListener(
        "blocked",
        () => {
          rejected = true;
          reject(new Error("Close other swarmie windows to update attachment storage."));
        },
        { once: true },
      );
    });
    this.database = pending;
    try {
      return await pending;
    } catch (error) {
      if (this.database === pending) {
        this.database = undefined;
      }
      throw error;
    }
  }
  async load(): Promise<CachedWorkspace | undefined> {
    if (!this.available()) {
      return undefined;
    }
    if (nativeClient()) {
      return this.saved(await this.native("client/cacheLoad", {}));
    }
    const database = await this.open();
    // Decode within the success callback so invalid cached data rejects instead of stranding this read.
    // oxlint-disable-next-line promise/avoid-new -- The native IDB request owns success/error settlement, including decoding failures.
    return await new Promise<CachedWorkspace | undefined>((resolve, reject) => {
      const transaction = database.transaction("workspace", "readonly");
      const request = transaction.objectStore("workspace").get(nodeKey(this.node));
      request.addEventListener(
        "success",
        () => {
          const value: unknown = request.result;
          try {
            resolve(this.saved(value));
          } catch (error) {
            reject(error instanceof Error ? error : new Error("Invalid cached workspace."));
          }
        },
        { once: true },
      );
      request.addEventListener(
        "error",
        () => {
          reject(request.error ?? new Error("Cannot read attachment storage."));
        },
        { once: true },
      );
    });
  }
  private saved(value: unknown): CachedWorkspace | undefined {
    if (value === undefined || value === null) {
      return undefined;
    }
    const saved = decodeCachedWorkspace(value);
    return validNodeIdentity(saved.node) && sameNode(saved.node, this.node) ? saved : undefined;
  }
  private async native(
    method: "client/cacheLoad" | "client/cacheSave",
    params: Record<string, unknown>,
  ): Promise<unknown> {
    const response = await clientRequest(
      "/client/cache",
      {
        jsonrpc: "2.0",
        id: crypto.randomUUID(),
        method,
        params: { node: this.node, ...params },
      },
      "",
      30_000,
    );
    const value: unknown = await response.json();
    return responseResult(value);
  }
  async save(value: CachedWorkspace): Promise<void> {
    if (nativeClient()) {
      await this.native("client/cacheSave", { value: { ...value, node: this.node } });
      return;
    }
    const database = await this.open();
    // A write settles at transaction completion; request success alone does not acknowledge durable storage.
    // oxlint-disable-next-line promise/avoid-new -- Native IDB transaction completion/error/abort owns write settlement and cancellation.
    await new Promise<void>((resolve, reject) => {
      const transaction = database.transaction("workspace", "readwrite");
      transaction.objectStore("workspace").put({ ...value, node: this.node }, nodeKey(this.node));
      transaction.addEventListener(
        "complete",
        () => {
          resolve();
        },
        { once: true },
      );
      transaction.addEventListener(
        "error",
        () => {
          reject(transaction.error ?? new Error("Cannot save attachment drafts."));
        },
        { once: true },
      );
      transaction.addEventListener(
        "abort",
        () => {
          reject(transaction.error ?? new Error("Attachment storage transaction was cancelled."));
        },
        { once: true },
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
    if (!saved) {
      return thread;
    }
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
    for (const key of edited.get(thread.id) ?? []) {
      Object.defineProperty(hydrated, key, {
        value: thread[key],
        enumerable: true,
        configurable: true,
        writable: true,
      });
    }
    return hydrated;
  });
}

function cacheAvailable() {
  return nativeClient() || typeof indexedDB !== "undefined";
}
