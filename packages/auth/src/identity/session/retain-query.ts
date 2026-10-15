import { hashKey, QueryObserver } from "@tanstack/react-query";
import type { QueryClient } from "@tanstack/react-query";

import type { createSessionQueryOptions } from "~/src/identity/session/query";

/** Keep one admitted session read alive across route observer gaps without starting public reads. */
export function retainSessionQuery(
  client: QueryClient,
  sessionQueryOptions: ReturnType<typeof createSessionQueryOptions>,
): () => void {
  const sessionHash = hashKey(sessionQueryOptions.queryKey);
  let release: (() => void) | undefined;
  const attach = () => {
    if (release || client.getQueryState(sessionQueryOptions.queryKey)?.status !== "success") {
      return;
    }
    // Pages own refresh policy; this observer only retains their in-flight read.
    const observer = new QueryObserver(client, { ...sessionQueryOptions, enabled: false });
    release = observer.subscribe(() => undefined);
  };
  const unsubscribe = client.getQueryCache().subscribe((event) => {
    if (event.query.queryHash !== sessionHash) {
      return;
    }
    if (event.type === "removed") {
      release?.();
      release = undefined;
    } else if (event.type === "updated") {
      attach();
    }
  });
  attach();
  return () => {
    unsubscribe();
    release?.();
    release = undefined;
  };
}
