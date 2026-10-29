import type { NodeConnection } from "@swarmie/plugin-connections-browser";

import { responseValue } from "./protocol";
import type { GitRequest, GitResponse } from "./protocol";

const responses: Record<GitRequest["kind"], GitResponse["kind"]> = {
  capability_info: "capability_info",
  discover: "repository",
  identify: "repository",
  reconcile: "receipt",
  status: "status",
  diff: "diff",
  file: "file",
  files: "files",
  history: "history",
  remotes: "remotes",
  mutate: "receipt",
  recover: "receipt",
  settings_get: "settings",
  settings_set: "settings",
  worktree_create: "worktree",
  archive_create: "archive",
  archive_restore: "archive",
  archive_list: "archives",
  cleanup_check: "cleanup",
  operations: "operations",
};

export const gitRequestKinds = Object.freeze(Object.keys(responses));

export type GitConnection = Pick<
  NodeConnection,
  "nodeId" | "principal" | "generation" | "isCurrent" | "get"
>;

/** A Git capability stays bound to one verified session, including during recovery. */
export class GitRemote {
  readonly identity: { nodeId: string; principal: string; generation: number };

  constructor(private readonly connection: GitConnection) {
    this.identity = Object.freeze({
      nodeId: connection.nodeId,
      principal: connection.principal,
      generation: connection.generation,
    });
  }

  async request(request: GitRequest): Promise<GitResponse> {
    this.check();
    const reference = await this.connection.get("swarmie.git", "main");
    try {
      this.check();
      const method: unknown = Reflect.get(reference, "request");
      if (typeof method !== "function") {
        throw new TypeError(
          "Git is unavailable on this node. Enable its native Git capability first.",
        );
      }
      const response: unknown = await Reflect.apply(method, undefined, [request]);
      this.check();
      const result = responseValue(response);
      if (
        result.kind === "operations" &&
        result.operations.some(
          (entry) =>
            entry.principal !== this.identity.principal ||
            !("target" in request) ||
            entry.target.common_dir !== request.target.common_dir,
        )
      ) {
        throw new Error("The Git recovery catalog belongs to another principal or repository.");
      }
      if (
        result.kind !== responses[request.kind] &&
        !(result.kind === "receipt" && "operation_id" in request)
      ) {
        throw new Error(
          "The Git node returned an unexpected operation result. Reconcile before continuing.",
        );
      }
      if (
        "receipt" in result &&
        (result.receipt.principal !== this.identity.principal ||
          !("operation_id" in request) ||
          result.receipt.operation_id !== request.operation_id)
      ) {
        throw new Error(
          "The Git receipt belongs to another operation or principal. Reconcile the original operation before continuing.",
        );
      }
      let returnedTarget = null;
      if (result.kind === "status") {
        returnedTarget = result.status.target;
      } else if ("receipt" in result) {
        returnedTarget = result.receipt.target;
      }
      if (
        returnedTarget !== null &&
        "target" in request &&
        (returnedTarget.root !== request.target.root ||
          returnedTarget.git_dir !== request.target.git_dir ||
          returnedTarget.common_dir !== request.target.common_dir)
      ) {
        throw new Error(
          "The Git node returned a different checkout. Refresh the original folder before continuing.",
        );
      }
      return result;
    } finally {
      reference.release();
    }
  }

  private check() {
    if (!this.connection.isCurrent()) {
      throw new Error(
        "The Git node session ended. Reconnect and refresh this checkout before continuing. An interrupted operation must be reconciled before retrying.",
      );
    }
  }
}
