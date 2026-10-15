import { useQuery } from "@tanstack/react-query";
import type { QueryKey } from "@tanstack/react-query";
import { useEffect } from "react";

import type { WorkspaceStore } from "../workspace";

/** Cache extension reads by their complete conversation and parameter identity. */
export function useExtensionQuery<T>(
  store: WorkspaceStore,
  queryKey: QueryKey,
  queryFn: () => Promise<T>,
  enabled = true,
) {
  const query = useQuery({ queryKey, queryFn, enabled, retry: false });
  useEffect(() => {
    if (query.error !== null) {
      store.setError(query.error);
    }
  }, [query.error, store]);
  const refresh = async () => {
    const result = await query.refetch();
    if (result.error !== null) {
      throw result.error;
    }
  };
  return { ...query, refresh };
}
