import type { QueryClient } from "@tanstack/react-query";

/** Every private capability shares a lifecycle while retaining its own query namespace. */
export const accountQueryKeys = { all: ["account"] as const };

type AccountResourceCleanup = () => void;

const accountResources = new WeakMap<QueryClient, Set<AccountResourceCleanup>>();

/** Attach non-Query account state to the same explicit lifecycle as private cached data. */
export function registerAccountResource(
  client: QueryClient,
  cleanup: AccountResourceCleanup,
): () => void {
  let resources = accountResources.get(client);
  if (!resources) {
    resources = new Set();
    accountResources.set(client, resources);
  }
  resources.add(cleanup);
  let registered = true;
  return () => {
    if (!registered) {
      return;
    }
    registered = false;
    resources.delete(cleanup);
  };
}

/** Cancel private reads before removing them so late responses cannot restore a former account. */
export async function clearAccountQueries(client: QueryClient): Promise<void> {
  const resources = accountResources.get(client);
  if (resources) {
    // oxlint-disable-next-line unicorn/no-useless-spread -- Cleanup callbacks can unregister other resources; iterate the original registration snapshot.
    for (const cleanup of [...resources]) {
      cleanup();
    }
  }
  await client.cancelQueries({ queryKey: accountQueryKeys.all });
  client.removeQueries({ queryKey: accountQueryKeys.all });
  client.getMutationCache().clear();
}
