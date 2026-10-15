import { QueryClient } from "@tanstack/react-query";

/** Browser-only cache: authentication never persists or resumes mutations after reconnecting. */
export function createQueryClient() {
  return new QueryClient({
    defaultOptions: {
      queries: {
        retry: false,
        refetchOnWindowFocus: false,
        refetchOnReconnect: false,
        staleTime: 0,
        gcTime: 0,
      },
      mutations: { retry: false, networkMode: "always", gcTime: 0 },
    },
  });
}
