export type RepositoryTarget = {
  root: string | null;
  git_dir: string;
  common_dir: string;
  bare: boolean;
  head: string | null;
  branch: string | null;
  fingerprint: string;
  index_fingerprint: string;
  observation_error: "output_limit" | null;
};
export type DiffMode = "unified" | "split";
export type StatusEntry = {
  path: string;
  original_path: string | null;
  index: string;
  worktree: string;
  kind: "tracked" | "untracked" | "ignored" | "unmerged";
};
export type GitStatus = { target: RepositoryTarget; entries: StatusEntry[] };
export type PageCursor = { offset: number; digest: string };
export type GitDiff = { text: string; truncated: boolean; mode: DiffMode };
export type GitCommit = {
  oid: string;
  parents: string[];
  author: string;
  authored_at: string;
  subject: string;
};
export type GitRemoteInfo = { name: string; fetch_urls: string[]; push_urls: string[] };
export type GitReceipt = {
  operation_id: string;
  principal: string;
  target: RepositoryTarget;
  state: "completed" | "failed" | "unknown" | "in_progress";
  message: string;
  head: string | null;
};
export type GitOperation =
  | { kind: "stage" | "unstage"; paths: string[] }
  | { kind: "commit"; message: string; reviewed_index: string; run_hooks: boolean }
  | { kind: "create_branch"; name: string }
  | { kind: "fetch"; remote: string }
  | { kind: "push"; remote: string; source_oid: string; destination_ref: string };
export type SetupScript = {
  enabled: boolean;
  shell: string;
  arguments: string[];
  script: string;
  working_directory: string;
};
export type GitSettingsValue = {
  diff_layout: DiffMode;
  include_local_changes: boolean;
  managed_root: string | null;
  branch_prefix: string;
  commit_guidance: string;
  run_commit_hooks: boolean;
  cleanup_enabled: boolean;
  retained_worktrees: number | null;
  setup_by_platform: Record<string, SetupScript>;
};
export type WorktreeRequest = {
  managed_root: string;
  destination: string;
  commit: string;
  include_paths: string[];
};
export type CreatedWorktree = {
  target: RepositoryTarget;
  source_head: string;
  preparation_error: string | null;
};
export type ArchiveReceipt = {
  archive_id: string;
  target: RepositoryTarget;
  head: string;
  files: number;
  restore_verified: boolean;
  restored_target: RepositoryTarget | null;
  cleanup_blocked_reason: string;
};
export type UnavailableEntry = { entry_id: string; message: string };
export type ManagedMutation =
  | { kind: "settings_set"; project_id?: string; settings: GitSettingsValue }
  | { kind: "worktree_create"; project_id?: string; request: WorktreeRequest }
  | { kind: "archive_create"; archive_id: string }
  | { kind: "archive_restore"; archive_id: string; managed_root: string; destination: string };
export type GitRequest =
  | { kind: "capability_info" }
  | { kind: "discover"; path: string }
  | { kind: "remotes"; target: RepositoryTarget }
  | { kind: "status" | "files"; target: RepositoryTarget; page?: PageCursor }
  | { kind: "diff"; target: RepositoryTarget; path: string | null; staged: boolean; mode: DiffMode }
  | { kind: "file"; target: RepositoryTarget; path: string; revision: string | null }
  | { kind: "history"; target: RepositoryTarget; limit: number }
  | { kind: "mutate"; operation_id: string; target: RepositoryTarget; operation: GitOperation }
  | { kind: "recover"; operation_id: string }
  | { kind: "settings_get"; target: RepositoryTarget; project_id?: string }
  | { kind: "archive_list"; target: RepositoryTarget; page?: PageCursor }
  | { kind: "operations"; target: RepositoryTarget; page?: PageCursor }
  | { kind: "cleanup_check"; target: RepositoryTarget; archive_id: string }
  | (ManagedMutation & { operation_id: string; target: RepositoryTarget });
export type GitResponse =
  | {
      kind: "capability_info";
      api_version: number;
      execution_available: boolean;
      operations: string[];
    }
  | { kind: "repository"; repository: RepositoryTarget | null }
  | { kind: "status"; status: GitStatus; next_page: PageCursor | null }
  | { kind: "diff"; diff: GitDiff }
  | { kind: "history"; commits: GitCommit[] }
  | { kind: "remotes"; remotes: GitRemoteInfo[] }
  | { kind: "files"; paths: string[]; next_page: PageCursor | null }
  | { kind: "file"; text: string; truncated: boolean }
  | { kind: "receipt"; receipt: GitReceipt }
  | { kind: "settings"; settings: GitSettingsValue; execution_platform: string }
  | { kind: "worktree"; worktree: CreatedWorktree; receipt: GitReceipt }
  | { kind: "archive"; archive: ArchiveReceipt; receipt: GitReceipt }
  | {
      kind: "archives";
      archives: ArchiveReceipt[];
      unavailable: UnavailableEntry[];
      next_page: PageCursor | null;
    }
  | {
      kind: "operations";
      operations: GitReceipt[];
      unavailable: UnavailableEntry[];
      next_page: PageCursor | null;
    }
  | {
      kind: "cleanup";
      eligibility: { eligible: boolean; changed_since_archive: boolean; reason: string };
    };

function invalid(): never {
  throw new Error("The Git node returned an invalid response. Reconnect to a compatible node.");
}
function isRecord(input: unknown): input is Record<string, unknown> {
  return typeof input === "object" && input !== null && !Array.isArray(input);
}
export function record(value: unknown): Record<string, unknown> {
  return isRecord(value) ? value : invalid();
}
function number(value: unknown): number {
  return typeof value === "number" && Number.isSafeInteger(value) ? value : invalid();
}
function setupValue(input: unknown): SetupScript {
  const value = record(input);
  return {
    enabled: boolean(value.enabled),
    shell: string(value.shell),
    arguments: array(value.arguments, string),
    script: string(value.script),
    working_directory: string(value.working_directory),
  };
}
export function settingsValue(input: unknown): GitSettingsValue {
  const value = record(input);
  const setup = record(value.setup_by_platform);
  const setupByPlatform: Record<string, SetupScript> = {};
  for (const [key, entry] of Object.entries(setup)) {
    Object.defineProperty(setupByPlatform, key, {
      value: setupValue(entry),
      enumerable: true,
      writable: true,
      configurable: true,
    });
  }
  return {
    diff_layout: choice(value.diff_layout, ["unified", "split"]),
    include_local_changes: boolean(value.include_local_changes),
    managed_root: nullableString(value.managed_root),
    branch_prefix: string(value.branch_prefix),
    commit_guidance: string(value.commit_guidance),
    run_commit_hooks: boolean(value.run_commit_hooks),
    cleanup_enabled: boolean(value.cleanup_enabled),
    retained_worktrees: value.retained_worktrees === null ? null : number(value.retained_worktrees),
    setup_by_platform: setupByPlatform,
  };
}
function archiveValue(input: unknown): ArchiveReceipt {
  const value = record(input);
  return {
    archive_id: string(value.archive_id),
    target: targetValue(value.target),
    head: string(value.head),
    files: number(value.files),
    restore_verified: boolean(value.restore_verified),
    restored_target: value.restored_target === null ? null : targetValue(value.restored_target),
    cleanup_blocked_reason: string(value.cleanup_blocked_reason),
  };
}
export function string(value: unknown): string {
  return typeof value === "string" ? value : invalid();
}
export function nullableString(value: unknown): string | null {
  return value === null ? null : string(value);
}
export function boolean(value: unknown): boolean {
  return typeof value === "boolean" ? value : invalid();
}
export function array<T>(value: unknown, parse: (entry: unknown) => T): T[] {
  return Array.isArray(value) ? value.map(parse) : invalid();
}
export function choice<T extends string>(value: unknown, values: readonly T[]): T {
  for (const candidate of values) {
    if (value === candidate) {
      return candidate;
    }
  }
  return invalid();
}
export function targetValue(input: unknown): RepositoryTarget {
  const value = record(input);
  return {
    root: nullableString(value.root),
    git_dir: string(value.git_dir),
    common_dir: string(value.common_dir),
    bare: boolean(value.bare),
    head: nullableString(value.head),
    branch: nullableString(value.branch),
    fingerprint: string(value.fingerprint),
    index_fingerprint: string(value.index_fingerprint),
    observation_error:
      value.observation_error === null
        ? null
        : choice(value.observation_error, ["output_limit"] as const),
  };
}
function entryValue(input: unknown): StatusEntry {
  const value = record(input);
  return {
    path: string(value.path),
    original_path: nullableString(value.original_path),
    index: string(value.index),
    worktree: string(value.worktree),
    kind: choice(value.kind, ["tracked", "untracked", "ignored", "unmerged"]),
  };
}
function pageValue(input: unknown): PageCursor | null {
  if (input === null) {
    return null;
  }
  const value = record(input);
  const offset = number(value.offset);
  if (offset <= 0) {
    return invalid();
  }
  const digest = string(value.digest);
  return digest.length > 0 ? { offset, digest } : invalid();
}
function commitValue(input: unknown): GitCommit {
  const value = record(input);
  return {
    oid: string(value.oid),
    parents: array(value.parents, string),
    author: string(value.author),
    authored_at: string(value.authored_at),
    subject: string(value.subject),
  };
}
function remoteValue(input: unknown): GitRemoteInfo {
  const value = record(input);
  return {
    name: string(value.name),
    fetch_urls: array(value.fetch_urls, string),
    push_urls: array(value.push_urls, string),
  };
}
export function receiptValue(input: unknown): GitReceipt {
  const value = record(input);
  return {
    operation_id: string(value.operation_id),
    principal: string(value.principal),
    target: targetValue(value.target),
    state: choice(value.state, ["completed", "failed", "unknown", "in_progress"]),
    message: string(value.message),
    head: nullableString(value.head),
  };
}
function unavailableValue(input: unknown): UnavailableEntry {
  const value = record(input);
  return { entry_id: string(value.entry_id), message: string(value.message) };
}
export function responseValue(input: unknown): GitResponse {
  const value = record(input);
  switch (value.kind) {
    case "capability_info": {
      return {
        kind: "capability_info",
        api_version: number(value.api_version),
        execution_available: boolean(value.execution_available),
        operations: array(value.operations, string),
      };
    }
    case "repository": {
      return {
        kind: "repository",
        repository: value.repository === null ? null : targetValue(value.repository),
      };
    }
    case "status": {
      const status = record(value.status);
      return {
        kind: "status",
        status: { target: targetValue(status.target), entries: array(status.entries, entryValue) },
        next_page: pageValue(value.next_page),
      };
    }
    case "diff": {
      const diff = record(value.diff);
      return {
        kind: "diff",
        diff: {
          text: string(diff.text),
          truncated: boolean(diff.truncated),
          mode: choice(diff.mode, ["unified", "split"]),
        },
      };
    }
    case "history": {
      return { kind: "history", commits: array(value.commits, commitValue) };
    }
    case "remotes": {
      return { kind: "remotes", remotes: array(value.remotes, remoteValue) };
    }
    case "files": {
      return {
        kind: "files",
        paths: array(value.paths, string),
        next_page: pageValue(value.next_page),
      };
    }
    case "file": {
      return { kind: "file", text: string(value.text), truncated: boolean(value.truncated) };
    }
    case "receipt": {
      return { kind: "receipt", receipt: receiptValue(value.receipt) };
    }
    case "settings": {
      return {
        kind: "settings",
        settings: settingsValue(value.settings),
        execution_platform: string(value.execution_platform),
      };
    }
    case "worktree": {
      const worktree = record(value.worktree);
      return {
        kind: "worktree",
        worktree: {
          target: targetValue(worktree.target),
          source_head: string(worktree.source_head),
          preparation_error: nullableString(worktree.preparation_error),
        },
        receipt: receiptValue(value.receipt),
      };
    }
    case "archive": {
      return {
        kind: "archive",
        archive: archiveValue(value.archive),
        receipt: receiptValue(value.receipt),
      };
    }
    case "archives": {
      return {
        kind: "archives",
        archives: array(value.archives, archiveValue),
        unavailable: array(value.unavailable, unavailableValue),
        next_page: pageValue(value.next_page),
      };
    }
    case "operations": {
      return {
        kind: "operations",
        operations: array(value.operations, receiptValue),
        unavailable: array(value.unavailable, unavailableValue),
        next_page: pageValue(value.next_page),
      };
    }
    case "cleanup": {
      const eligibility = record(value.eligibility);
      return {
        kind: "cleanup",
        eligibility: {
          eligible: boolean(eligibility.eligible),
          changed_since_archive: boolean(eligibility.changed_since_archive),
          reason: string(eligibility.reason),
        },
      };
    }
    default: {
      return invalid();
    }
  }
}
