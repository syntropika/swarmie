import { useQuery } from "@tanstack/react-query";
import { useEffect, useId, useState } from "react";

import type { WorkspaceStore } from "~/src/workspace";

/** Each mounted search owns its queries; results never cross active worktrees or input revisions. */
export function useWorkspaceSearchFiles(store: WorkspaceStore, query: string, enabled: boolean) {
  const owner = useId();
  const [settledQuery, setSettledQuery] = useState(query);
  useEffect(() => {
    const timer = setTimeout(() => {
      setSettledQuery(query);
    }, 120);
    return () => {
      clearTimeout(timer);
    };
  }, [query]);
  const thread = store.active();
  const worktreeId = thread?.worktreeId ?? "";
  const available = worktreeId.length > 0;
  const ready = query === settledQuery;
  const files = useQuery({
    queryKey: ["workspace-search-files", owner, thread?.id, worktreeId, settledQuery],
    enabled: enabled && available && ready,
    queryFn: async () => {
      if (store.active()?.worktreeId !== worktreeId) {
        return [];
      }
      return await store.findFiles(settledQuery);
    },
    retry: false,
    gcTime: 0,
  });
  return {
    available,
    paths: enabled && ready ? (files.data ?? []) : [],
    loading: enabled && available && (!ready || files.isPending),
    error: enabled && ready ? files.error : null,
  };
}
