import type { Snapshot, UiStore } from "@swarmie/client-ui";

import type {
  ProviderAccount,
  ProviderAuthorizationSession,
  ProvidersState,
  ProviderConfigurationApi,
  ProviderAuthorizationApi,
  ProviderDiscoveryApi,
  ModelDefaultsApi,
} from "./index";
import {
  auxiliarySelection,
  defaultSelection,
  resolveSelection,
  selectAccount,
  selectModel,
} from "./selection";
import type { ProviderSelection } from "./selection";

/** One node adapter supplies provider configuration and its subscribed account/model catalog. */
export type ProvidersBackend = {
  catalog: Snapshot<Readonly<ProvidersState>>;
  configuration?: ProviderConfigurationApi;
  authorization?: ProviderAuthorizationApi;
  discovery?: ProviderDiscoveryApi;
  defaults?: ModelDefaultsApi;
};
export type ProvidersApi = ReturnType<typeof createProvidersApi>;

function completedAccount(accounts: ProviderAccount[], account: ProviderAccount): ProviderAccount {
  const previous = accounts.find((entry) => entry.id === account.id);
  return account.modelsTruncated === true &&
    previous !== undefined &&
    previous.modelsTruncated !== true
    ? { ...account, models: previous.models, modelsTruncated: false }
    : account;
}

/** Uses the owner's Tembra State; there is no separate model catalog or state engine. */
export function createProvidersApi(
  state: UiStore<Readonly<ProvidersState>>,
  commit: (change: (current: Readonly<ProvidersState>) => ProvidersState) => Promise<unknown>,
  backend?: ProvidersBackend,
) {
  let closed = false;
  const sessions = new Set<() => void>();
  const adoptCatalog = () => {
    if (!closed && backend && backend.catalog !== state) {
      state.set(backend.catalog.getSnapshot());
    }
  };
  const detach = backend?.catalog === state ? undefined : backend?.catalog.subscribe(adoptCatalog);
  adoptCatalog();
  function assertOpen() {
    if (closed) {
      throw new Error("Providers is closed.");
    }
  }
  async function connected(account: ProviderAccount, scope = state.getSnapshot().scope) {
    assertOpen();
    await commit((current) =>
      current.scope === scope
        ? {
            ...current,
            accounts: [
              ...current.accounts.filter((entry) => entry.id !== account.id),
              completedAccount(current.accounts, account),
            ],
          }
        : current,
    );
    if (state.getSnapshot().scope !== scope) {
      throw new Error("The provider result belongs to an earlier node session.");
    }
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
    scope: ProvidersState["scope"],
  ): ProviderAuthorizationSession {
    let lastAccount: ProviderAccount | undefined;
    let disposed = false;
    const changed = () => {
      const value = session.getSnapshot();
      if (!closed && !disposed && value.kind === "connected" && value.account !== lastAccount) {
        lastAccount = value.account;
        connected(value.account, scope).catch(publishFailed);
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
      ...(session.retry === undefined ? {} : { retry: session.retry }),
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
          const { scope } = state.getSnapshot();
          const account = await configuration.configure(input, signal);
          await connected(account, scope);
          return account;
        },
      }
    : null;
  const authorization = backend?.authorization;
  const authorizationApi: ProviderAuthorizationApi | null = authorization
    ? {
        async begin(input, signal) {
          assertOpen();
          const { scope } = state.getSnapshot();
          const session = await authorization.begin(input, signal);
          if (closed) {
            session.dispose();
            throw new Error("Providers is closed.");
          }
          return observeAuthorization(session, scope);
        },
      }
    : null;
  const discovery = backend?.discovery;
  const defaultsBackend = backend?.defaults;
  const defaultsApi: ModelDefaultsApi | null = defaultsBackend
    ? {
        async save(accountId, defaults, signal) {
          assertOpen();
          signal.throwIfAborted();
          const { scope } = state.getSnapshot();
          const saved = await defaultsBackend.save(accountId, defaults, signal);
          assertOpen();
          signal.throwIfAborted();
          if (state.getSnapshot().scope !== scope) {
            throw new Error("The model defaults belong to an earlier node session.");
          }
          await commit((current) =>
            current.scope === scope
              ? {
                  ...current,
                  accounts: current.accounts.map((account) =>
                    account.id === accountId ? { ...account, defaults: saved } : account,
                  ),
                }
              : current,
          );
          if (state.getSnapshot().scope !== scope) {
            throw new Error("The model defaults belong to an earlier node session.");
          }
          return saved;
        },
      }
    : null;
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
    defaults: defaultsApi,
    auxiliarySelection: (accountId: string) => {
      const account = state.getSnapshot().accounts.find((entry) => entry.id === accountId);
      return account ? auxiliarySelection(account) : null;
    },
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
