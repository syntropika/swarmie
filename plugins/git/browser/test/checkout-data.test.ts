import { expect, test } from "bun:test";

import { readCheckout } from "../src/checkout-data";
import type { GitRequest, GitResponse, GitSettingsValue, RepositoryTarget } from "../src/protocol";

const target: RepositoryTarget = {
  root: "/checkout",
  git_dir: "/checkout/.git",
  common_dir: "/checkout/.git",
  bare: false,
  head: "abc",
  branch: "main",
  fingerprint: "captured",
  index_fingerprint: "index",
  observation_error: null,
};
const settings: GitSettingsValue = {
  diff_layout: "unified",
  include_local_changes: false,
  managed_root: null,
  branch_prefix: "",
  commit_guidance: "",
  cleanup_enabled: false,
  retained_worktrees: null,
  setup_by_platform: {},
};
function fixture(read: (request: GitRequest) => GitResponse) {
  return {
    request: async (request: GitRequest): Promise<GitResponse> => {
      if (request.kind === "discover") {
        return { kind: "repository", repository: target };
      }
      if (request.kind === "settings_get") {
        return { kind: "settings", settings, execution_platform: "linux" };
      }
      return read(request);
    },
  };
}
const entry = (path: string) => ({
  path,
  original_path: null,
  index: ".",
  worktree: ".",
  kind: "untracked" as const,
});

test("all inventory pages arrive before a complete checkout is published", async () => {
  const remote = fixture((request) => {
    if (request.kind === "status") {
      return {
        kind: "status",
        status: { target, entries: [entry(request.page ? "b" : "a")] },
        next_page: request.page ? null : { offset: 1, digest: "status" },
      };
    }
    if (request.kind === "files") {
      return {
        kind: "files",
        paths: request.page ? ["b"] : ["a"],
        next_page: request.page ? null : { offset: 1, digest: "files" },
      };
    }
    if (request.kind === "history") {
      return { kind: "history", commits: [] };
    }
    return { kind: "remotes", remotes: [] };
  });
  const result = await readCheckout(remote, "/checkout", undefined);
  expect(result?.checkout?.files).toEqual({ state: "ready", value: ["a", "b"] });
  expect(result?.checkout?.status.entries.map((value) => value.path)).toEqual(["a", "b"]);
  expect(result?.settings).toEqual(settings);
});

test("failed optional inventories leave changes and settings usable with explicit errors", async () => {
  const remote = fixture((request) => {
    if (request.kind === "status") {
      return {
        kind: "status",
        status: { target, entries: [entry("changed.txt")] },
        next_page: null,
      };
    }
    throw new Error(`${request.kind} unavailable`);
  });
  const result = await readCheckout(remote, "/checkout", "project");
  expect(result?.checkout?.status.entries).toHaveLength(1);
  expect(result?.settings).toEqual(settings);
  expect(result?.checkout?.files).toEqual({ state: "unavailable", error: "files unavailable" });
  expect(result?.checkout?.commits).toEqual({ state: "unavailable", error: "history unavailable" });
  expect(result?.checkout?.remotes).toEqual({ state: "unavailable", error: "remotes unavailable" });
});

test("a failed final file page never appears as a complete partial inventory", async () => {
  const remote = fixture((request) => {
    if (request.kind === "status") {
      return { kind: "status", status: { target, entries: [] }, next_page: null };
    }
    if (request.kind === "files" && !request.page) {
      return { kind: "files", paths: ["first"], next_page: { offset: 1, digest: "files" } };
    }
    throw new Error("checkout changed");
  });
  const result = await readCheckout(remote, "/checkout", undefined);
  expect(result?.checkout?.files).toEqual({ state: "unavailable", error: "checkout changed" });
});

test("a nonadvancing continuation is rejected rather than looping forever", async () => {
  const remote = fixture((request) => {
    if (request.kind === "status") {
      return {
        kind: "status",
        status: { target, entries: [entry("a")] },
        next_page: { offset: 1, digest: "status" },
      };
    }
    return { kind: "files", paths: [], next_page: null };
  });
  const result = await readCheckout(remote, "/checkout", undefined).catch(
    (error: unknown) => error,
  );
  expect(result).toBeInstanceOf(Error);
  expect(result instanceof Error ? result.message : "unexpected success").toContain(
    "continuation is inconsistent",
  );
});

test("an oversized observation still publishes settings without inventing a clean checkout", async () => {
  const calls: string[] = [];
  const remote = {
    request: async (request: GitRequest): Promise<GitResponse> => {
      calls.push(request.kind);
      if (request.kind === "discover") {
        return {
          kind: "repository",
          repository: {
            ...target,
            fingerprint: "",
            index_fingerprint: "",
            observation_error: "output_limit",
          },
        };
      }
      if (request.kind === "settings_get") {
        return { kind: "settings", settings, execution_platform: "linux" };
      }
      throw new Error("An incomplete observation cannot be used for content operations");
    },
  };
  const result = await readCheckout(remote, "/checkout", undefined);
  expect(result?.checkout).toBeNull();
  expect(result?.settings).toEqual(settings);
  expect(result?.error).toContain("writes are unavailable");
  expect(calls).toEqual(["discover", "settings_get"]);
});
