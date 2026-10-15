import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createRouter } from "@tanstack/react-router";

import { routeTree } from "~/.tanstack/routeTree.gen";

/** Create an isolated router for each server request and browser application. */
export function getRouter() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false, networkMode: "always", gcTime: 0 },
    },
  });
  return createRouter({
    routeTree,
    trailingSlash: "always",
    scrollRestoration: true,
    Wrap: ({ children }) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    ),
  });
}

declare module "@tanstack/react-router" {
  // oxlint-disable-next-line typescript/consistent-type-definitions -- TanStack registers application types through interface declaration merging.
  interface Register {
    router: ReturnType<typeof getRouter>;
  }
}

declare module "@tanstack/react-start" {
  // oxlint-disable-next-line typescript/consistent-type-definitions -- TanStack registers application types through interface declaration merging.
  interface Register {
    ssr: true;
    router: ReturnType<typeof getRouter>;
  }
}
