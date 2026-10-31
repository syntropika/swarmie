import type { Connections, NodeConnection } from "@swarmie/plugin-browser/connections";
import type { DirectoryBrowser, DirectoryPage } from "@swarmie/plugin-projects-browser";
import type { DynamicReference } from "@syntropika/tembra-sdk";

import { callRemote } from "./remote";

function record(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
function path(value: unknown): value is string {
  return (
    typeof value === "string" && value.length > 0 && value.length <= 4096 && !value.includes("\0")
  );
}
function optionalPath(value: unknown): value is string | null {
  return value === null || path(value);
}
export function directoryPage(value: unknown): DirectoryPage {
  if (
    !record(value) ||
    !path(value.path) ||
    !optionalPath(value.parent) ||
    !optionalPath(value.next) ||
    !Array.isArray(value.directories) ||
    value.directories.length > 64
  ) {
    throw new Error("The node returned an invalid directory page.");
  }
  const directories = value.directories.map((entry: unknown) => {
    if (!record(entry) || !path(entry.name) || !path(entry.path)) {
      throw new Error("The node returned an invalid directory entry.");
    }
    return { name: entry.name, path: entry.path };
  });
  return { path: value.path, parent: value.parent, directories, next: value.next };
}

type Binding = { connection: NodeConnection; peer: DynamicReference };

/** One picker retains one ordinary Reference until it closes. Node replacement fences results. */
export function createDirectoryBrowser(connections: Connections): DirectoryBrowser {
  return {
    open(nodeId) {
      let closed = false;
      let pending: Promise<Binding> | undefined;
      let binding: Binding | undefined;
      async function acquire(): Promise<Binding> {
        if (closed) {
          throw new Error("Folder browsing has closed.");
        }
        if (pending) {
          return await pending;
        }
        const operation = async () => {
          const connection = await connections.open(nodeId);
          if (closed) {
            throw new Error("Folder browsing has closed.");
          }
          const peer = await connection.get("swarmie.directories", "main");
          if (closed || !connection.isCurrent()) {
            peer.release();
            throw new Error("The node session changed. Open the folder again.");
          }
          binding = { connection, peer };
          return binding;
        };
        pending = operation();
        try {
          return await pending;
        } catch (error: unknown) {
          pending = undefined;
          throw error;
        }
      }
      return {
        async list(request, signal) {
          signal.throwIfAborted();
          const aborted = Promise.withResolvers<never>();
          const cancel = () => {
            aborted.reject(new DOMException("Folder browsing cancelled.", "AbortError"));
          };
          signal.addEventListener("abort", cancel, { once: true });
          const operation = async () => {
            const { connection, peer } = await acquire();
            signal.throwIfAborted();
            if (closed || !connection.isCurrent()) {
              throw new Error("The node session changed. Open the folder again.");
            }
            const result = await callRemote(
              peer,
              "list",
              [request],
              "This node does not support folder browsing.",
            );
            signal.throwIfAborted();
            if (closed || !connection.isCurrent()) {
              throw new Error("The node session changed. Open the folder again.");
            }
            return directoryPage(result);
          };
          try {
            return await Promise.race([operation(), aborted.promise]);
          } finally {
            signal.removeEventListener("abort", cancel);
          }
        },
        close() {
          if (!closed) {
            closed = true;
            binding?.peer.release();
          }
        },
      };
    },
  };
}
