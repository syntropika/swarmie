import type {
  GitCommit,
  GitRemoteInfo,
  GitRequest,
  GitResponse,
  GitSettingsValue,
  GitStatus,
  PageCursor,
  RepositoryTarget,
} from "./protocol";

export type Resource<T> = { state: "ready"; value: T } | { state: "unavailable"; error: string };
export type Checkout = {
  target: RepositoryTarget;
  status: GitStatus;
  files: Resource<string[]>;
  commits: Resource<GitCommit[]>;
  remotes: Resource<GitRemoteInfo[]>;
};
export type CheckoutSnapshot = {
  checkout: Checkout | null;
  settings: GitSettingsValue;
  platform: string;
  error: string | null;
};
type Requester = { request: (request: GitRequest) => Promise<GitResponse> };

export function errorMessage(error: unknown) {
  return error instanceof Error
    ? error.message
    : "The Git operation failed. Refresh the checkout before continuing.";
}

async function resource<T>(read: () => Promise<T>): Promise<Resource<T>> {
  try {
    return { state: "ready", value: await read() };
  } catch (error: unknown) {
    return { state: "unavailable", error: errorMessage(error) };
  }
}

/** Publish an inventory only after every page of the same observation has arrived. */
async function inventory(
  remote: Requester,
  target: RepositoryTarget,
  kind: "status" | "files",
): Promise<{ paths: string[]; status: GitStatus | null }> {
  const paths: string[] = [];
  let status: GitStatus | null = null;
  async function readPage(page: PageCursor | null): Promise<void> {
    const response = await remote.request({ kind, target, ...(page === null ? {} : { page }) });
    if (response.kind !== kind || (response.kind !== "status" && response.kind !== "files")) {
      throw new Error("Unexpected Git inventory response.");
    }
    if (response.kind === "status") {
      if (status === null) {
        status = { target: response.status.target, entries: [] };
      } else if (status.target.fingerprint !== response.status.target.fingerprint) {
        throw new Error("The checkout changed during pagination. Refresh before continuing.");
      }
      status.entries.push(...response.status.entries);
    } else {
      paths.push(...response.paths);
    }
    const offset = response.kind === "status" ? status?.entries.length : paths.length;
    const next = response.next_page;
    if (
      next !== null &&
      (next.offset !== offset ||
        next.offset <= (page?.offset ?? 0) ||
        (page !== null && next.digest !== page.digest))
    ) {
      throw new Error("The Git inventory continuation is inconsistent. Refresh before continuing.");
    }
    if (next !== null) {
      await readPage(next);
    }
  }
  await readPage(null);
  return { paths, status };
}

export async function readCheckout(
  remote: Requester | null,
  path: string,
  projectId: string | undefined,
): Promise<CheckoutSnapshot | null> {
  if (remote === null) {
    return null;
  }
  const discovered = await remote.request({ kind: "discover", path });
  if (discovered.kind !== "repository") {
    throw new Error("Unexpected Git discovery response.");
  }
  if (discovered.repository === null) {
    return null;
  }
  const target = discovered.repository;
  if (target.observation_error !== null) {
    const preferences = await remote.request({
      kind: "settings_get",
      target,
      ...(projectId === undefined ? {} : { project_id: projectId }),
    });
    if (preferences.kind !== "settings") {
      throw new Error("Unexpected Git settings response.");
    }
    return {
      checkout: null,
      settings: preferences.settings,
      platform: preferences.execution_platform,
      error:
        "The repository inventory exceeds this node's observation limit. Settings remain readable; changes and writes are unavailable until a complete observation can be captured.",
    };
  }
  const [observation, files, commits, remotes, preferences] = await Promise.all([
    inventory(remote, target, "status"),
    resource(async () => {
      const response = await inventory(remote, target, "files");
      return response.paths;
    }),
    resource(async () => {
      const response = await remote.request({ kind: "history", target, limit: 30 });
      if (response.kind !== "history") {
        throw new Error("Unexpected Git history response.");
      }
      return response.commits;
    }),
    resource(async () => {
      const response = await remote.request({ kind: "remotes", target });
      if (response.kind !== "remotes") {
        throw new Error("Unexpected Git remotes response.");
      }
      return response.remotes;
    }),
    remote.request({
      kind: "settings_get",
      target,
      ...(projectId === undefined ? {} : { project_id: projectId }),
    }),
  ]);
  if (observation.status === null || preferences.kind !== "settings") {
    throw new Error("Unexpected Git checkout response.");
  }
  return {
    checkout: {
      target: observation.status.target,
      status: observation.status,
      files,
      commits,
      remotes,
    },
    settings: preferences.settings,
    platform: preferences.execution_platform,
    error: null,
  };
}
