import type { Snapshot, UiStore } from "@swarmie/client-ui";

import type {
  ProviderAccount,
  ProviderAuthorizationSession,
  ProvidersState,
  ProviderConfigurationApi,
  ProviderAuthorizationApi,
  ProviderDiscoveryApi,
} from "./index";
import { defaultSelection, resolveSelection, selectAccount, selectModel } from "./selection";
import type { ProviderSelection } from "./selection";

/** One node adapter supplies provider configuration and its subscribed account/model catalog. */
export type ProvidersBackend = {
  catalog: Snapshot<Readonly<ProvidersState>>;
  configuration?: ProviderConfigurationApi;
  authorization?: ProviderAuthorizationApi;
  discovery?: ProviderDiscoveryApi;
};
export type ProvidersApi = ReturnType<typeof createProvidersApi>;

/** Uses the owner's Tembra State; there is no separate model catalog or state engine. */
export function createProvidersApi(
  state: UiStore<Readonly<ProvidersState>>,
  commit: (change: (current: Readonly<ProvidersState>) => ProvidersState) => Promise<unknown>,
  backend?: ProvidersBackend,
) {
  let closed = false;
  const sessions = new Set<() => void>();
  const adoptCatalog = () => {
    if (!closed && backend) {
      state.set(backend.catalog.getSnapshot());
    }
  };
  const detach = backend?.catalog.subscribe(adoptCatalog);
  adoptCatalog();
  function assertOpen() {
    if (closed) {
      throw new Error("Providers is closed.");
    }
  }
  async function connected(account: ProviderAccount) {
    assertOpen();
    await commit((current) => ({
      ...current,
      accounts: [...current.accounts.filter((entry) => entry.id !== account.id), account],
    }));
  }
  function publishFailed(error: unknown) {
    if (!closed) {
      state.update((current) => ({
        ...current,
        error: error instanceof Error ? error.message : String(error),
      }));
    }
  }
  function observeAuthorization(
    session: ProviderAuthorizationSession,
  ): ProviderAuthorizationSession {
    let lastAccount: ProviderAccount | undefined;
    let disposed = false;
    const changed = () => {
      const value = session.getSnapshot();
      if (!closed && !disposed && value.kind === "connected" && value.account !== lastAccount) {
        lastAccount = value.account;
        connected(value.account).catch(publishFailed);
      }
    };
    const unsubscribe = session.subscribe(changed);
    const dispose = () => {
      if (!disposed) {
        disposed = true;
        unsubscribe();
        sessions.delete(dispose);
        session.dispose();
      }
    };
    sessions.add(dispose);
    changed();
    return {
      getSnapshot: session.getSnapshot,
      subscribe: session.subscribe,
      async complete(callbackUrl) {
        await session.complete(callbackUrl);
      },
      async cancel() {
        await session.cancel();
      },
      dispose,
    };
  }
  const configuration = backend?.configuration;
  const configurationApi: ProviderConfigurationApi | null = configuration
    ? {
        async configure(input, signal) {
          assertOpen();
          const account = await configuration.configure(input, signal);
          await connected(account);
          return account;
        },
      }
    : null;
  const authorization = backend?.authorization;
  const authorizationApi: ProviderAuthorizationApi | null = authorization
    ? {
        async begin(input, signal) {
          assertOpen();
          const session = await authorization.begin(input, signal);
          if (closed) {
            session.dispose();
            throw new Error("Providers is closed.");
          }
          return observeAuthorization(session);
        },
      }
    : null;
  const discovery = backend?.discovery;
  const discoveryApi: ProviderDiscoveryApi | null = discovery
    ? {
        async detect(signal) {
          assertOpen();
          return await discovery.detect(signal);
        },
      }
    : null;
  return {
    state,
    configuration: configurationApi,
    authorization: authorizationApi,
    discovery: discoveryApi,
    defaultSelection: () => defaultSelection(state.getSnapshot()),
    selectAccount: (accountId: string, current: ProviderSelection) =>
      selectAccount(state.getSnapshot(), accountId, current),
    selectModel: (current: ProviderSelection, modelId: string) =>
      selectModel(state.getSnapshot(), current, modelId),
    resolve: (selection: ProviderSelection) => resolveSelection(state.getSnapshot(), selection),
    dispose: () => {
      closed = true;
      detach?.();
      for (const dispose of sessions) {
        dispose();
      }
    },
  };
}
