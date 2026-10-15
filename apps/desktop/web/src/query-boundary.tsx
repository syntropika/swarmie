import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { useEffect, useMemo, useRef } from "react";
import type { ReactNode } from "react";

import { startAction } from "~/web/src/action-boundary";

async function disposeClient(client: QueryClient) {
  try {
    await client.cancelQueries();
  } finally {
    client.clear();
  }
}
/** The parent keys this boundary by retained node identity; no data crosses node changes. */
export function NodeQueryBoundary({ children }: { children: ReactNode }) {
  const client = useMemo(
    () =>
      new QueryClient({
        defaultOptions: { queries: { retry: false }, mutations: { retry: false, gcTime: 0 } },
      }),
    [],
  );
  const generation = useRef(0);
  useEffect(() => {
    generation.current += 1;
    return () => {
      generation.current += 1;
      const disposedGeneration = generation.current;
      // StrictMode reuses this client for its development remount; only final disposal owns cancellation.
      queueMicrotask(() => {
        if (generation.current !== disposedGeneration) {
          return;
        }
        startAction(disposeClient(client), () => {
          client.clear();
        });
      });
    };
  }, [client]);
  return <QueryClientProvider client={client}>{children}</QueryClientProvider>;
}
