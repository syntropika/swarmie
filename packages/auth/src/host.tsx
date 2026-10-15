import type { SwarmieClient } from "@swarmie/api";
import { QueryClientProvider } from "@tanstack/react-query";
import type { QueryClient } from "@tanstack/react-query";
import { createContext, useContext, useMemo } from "react";
import type { ComponentProps, ReactNode } from "react";

import { createAuthEffects } from "~/src/identity/authentication/effects";
import type { AuthEffects } from "~/src/identity/authentication/effects";
import { createSessionQueryOptions } from "~/src/identity/session/query";
import { observeBackgroundTask } from "~/src/platform/background-task";

export type AuthDestination =
  | "/"
  | "/login"
  | "/signup"
  | "/reset-password"
  | "/invitations/accept";
export type AuthNavigation = { to: AuthDestination; replace?: boolean };
export type AuthenticationHost = {
  client: SwarmieClient;
  queryClient: QueryClient;
  onError: (error: unknown) => void;
  siteUrl: string;
  navigate: (destination: AuthNavigation) => Promise<void>;
};
type AuthenticationContext = AuthenticationHost & { effects: AuthEffects };
const Context = createContext<AuthenticationContext | undefined>(undefined);

/** The host owns transport, navigation and account cache lifetime; no global browser client is created. */
export function AuthenticationProvider({
  host,
  children,
}: {
  host: AuthenticationHost;
  children: ReactNode;
}) {
  const value = useMemo(() => ({ ...host, effects: createAuthEffects(host.client) }), [host]);
  return (
    <QueryClientProvider client={host.queryClient}>
      <Context value={value}>{children}</Context>
    </QueryClientProvider>
  );
}
export function useAuthentication(): AuthenticationContext {
  const value = useContext(Context);
  if (!value) {
    throw new Error("AuthenticationProvider is required");
  }
  return value;
}
export function useNavigate() {
  const host = useAuthentication();
  return useMemo(
    () => async (destination: AuthNavigation) => {
      try {
        await host.navigate(destination);
      } catch (error) {
        host.onError(error);
        throw error;
      }
    },
    [host],
  );
}
export function useSessionQueryOptions() {
  const { effects } = useAuthentication();
  return useMemo(() => createSessionQueryOptions(effects), [effects]);
}

/** Preserve normal modified-click links; the host handles ordinary in-client navigation. */
export function Link({
  to,
  replace,
  onClick,
  children,
  ...props
}: Omit<ComponentProps<"a">, "href"> & AuthNavigation) {
  const navigate = useNavigate();
  return (
    <a
      {...props}
      href={to}
      onClick={(event) => {
        onClick?.(event);
        if (
          event.defaultPrevented ||
          event.button !== 0 ||
          event.metaKey ||
          event.ctrlKey ||
          event.shiftKey ||
          event.altKey ||
          props.target === "_blank"
        ) {
          return;
        }
        event.preventDefault();
        observeBackgroundTask(navigate({ to, ...(replace === undefined ? {} : { replace }) }));
      }}
    >
      {children}
    </a>
  );
}
