import type { Snapshot, UiStore } from "@swarmie/client-ui";
import { errorMessage } from "@swarmie/client-ui";
import type {
  Connections,
  ConnectionsState,
  NodeConnection,
} from "@swarmie/plugin-browser/connections";
import type { ProvidersBackend, ProvidersState } from "@swarmie/plugin-providers-browser";
import { createProvidersApi } from "@swarmie/plugin-providers-browser";
import type { DynamicReference } from "@syntropika/tembra-sdk";

import { readNodePages } from "./node-pages";
import { openProviderAuthorization } from "./provider-authorization";
import { modelDefaults, providerAccount, providerCatalog } from "./provider-values";
import { callRemote } from "./remote";

const unavailable = "The node provider service is unavailable.";

function mergeCatalog(pages: ReturnType<typeof providerCatalog>[]) {
  const accounts = new Map<string, ReturnType<typeof providerAccount>>();
  const choices = new Map<string, ReturnType<typeof providerCatalog>["choices"][number]>();
  for (const page of pages) {
    for (const account of page.accounts) {
      const previous = accounts.get(account.id);
      if (previous === undefined) {
        accounts.set(account.id, { ...account, modelsTruncated: false });
        continue;
      }
      if (
        previous.providerId !== account.providerId ||
        previous.label !== account.label ||
        previous.providerLabel !== account.providerLabel ||
        previous.authorizationMethod !== account.authorizationMethod ||
        JSON.stringify(previous.defaults) !== JSON.stringify(account.defaults)
      ) {
        throw new Error("The provider catalog changed while loading.");
      }
      const models = new Map(previous.models.map((model) => [model.id, model]));
      for (const model of account.models) {
        models.set(model.id, model);
      }
      accounts.set(account.id, {
        ...account,
        models: [...models.values()],
        modelsTruncated: false,
      });
    }
    for (const choice of page.choices) {
      choices.set(choice.id, choice);
    }
  }
  return { accounts: [...accounts.values()], choices: [...choices.values()] };
}

async function loadCatalog(reference: DynamicReference, current: () => boolean) {
  const pages = await readNodePages(
    async (cursor) => {
      const page = providerCatalog(await callRemote(reference, "list", [cursor], unavailable));
      if (!current()) {
        throw new Error("The provider catalog belongs to an earlier node session.");
      }
      return page;
    },
    { reason: "providers.catalog_changed", current },
  );
  return mergeCatalog(pages);
}

/** Projects one verified node session into the existing Providers Tembra State. */
export function createNodeProvidersBackend(
  state: UiStore<Readonly<ProvidersState>>,
  nodes: Snapshot<Readonly<ConnectionsState>>,
  connections: Connections,
) {
  let closed = false;
  let epoch = 0;
  let identity = "";
  let peer: DynamicReference | null = null;
  let connection: NodeConnection | null = null;
  let authorizationScope = new AbortController();
  let loading = 0;
  function current(version: number) {
    return !closed && epoch === version && connection?.isCurrent() === true;
  }
  async function load(nodeId: string, version: number) {
    loading += 1;
    const request = loading;
    let reference: DynamicReference | null = null;
    try {
      const session = await connections.open(nodeId);
      if (closed || epoch !== version) {
        return;
      }
      connection = session;
      reference = await session.get("swarmie.providers.settings", "main");
      if (!current(version)) {
        return;
      }
      const catalog = await loadCatalog(reference, () => current(version));
      if (current(version) && request === loading) {
        peer?.release();
        peer = reference;
        reference = null;
        state.update((value) => ({ ...value, ...catalog, ready: true, error: null }));
      }
    } catch (error: unknown) {
      if (!closed && epoch === version && request === loading) {
        state.update((value) => ({ ...value, ready: false, error: errorMessage(error) }));
      }
    } finally {
      reference?.release();
    }
  }
  function changed() {
    const value = nodes.getSnapshot();
    const node = value.nodes.find((entry) => entry.id === value.selected);
    const session = node === undefined ? undefined : value.sessions[node.id];
    const next = `${node?.id ?? ""}:${node?.principal ?? ""}:${session?.generation ?? 0}:${session?.phase ?? ""}`;
    if (identity === next) {
      return;
    }
    identity = next;
    epoch += 1;
    authorizationScope.abort();
    authorizationScope = new AbortController();
    peer?.release();
    peer = null;
    connection = null;
    const scope =
      node === undefined
        ? null
        : { nodeId: node.id, principal: node.principal, generation: session?.generation ?? 0 };
    state.set({ scope, accounts: [], choices: [], ready: false, error: null });
    if (node !== undefined && session?.phase === "connected") {
      load(node.id, epoch).catch(() => undefined);
    }
  }
  const backend: ProvidersBackend = {
    catalog: state,
    defaults: {
      async save(accountId, defaults, signal) {
        signal.throwIfAborted();
        const version = epoch;
        const reference = peer;
        if (reference === null || !current(version)) {
          throw new Error("Connect a node before saving model defaults.");
        }
        const saved = modelDefaults(
          await callRemote(reference, "set_defaults", [{ accountId, defaults }], unavailable),
        );
        signal.throwIfAborted();
        if (!current(version)) {
          throw new Error("The node changed while saving. Reload its settings before retrying.");
        }
        return saved;
      },
    },
    authorization: {
      async begin(input, signal) {
        const version = epoch;
        const session = connection;
        if (session === null || !current(version) || input.method !== "oauth-browser") {
          throw new Error("Connect a node before signing in to a provider.");
        }
        const authorization = await openProviderAuthorization(
          session,
          input.providerId,
          AbortSignal.any([signal, authorizationScope.signal]),
          () => current(version),
          input.accountId,
        );
        let refreshed = false;
        const authorizationChanged = () => {
          if (!refreshed && authorization.getSnapshot().kind === "connected" && current(version)) {
            refreshed = true;
            load(session.nodeId, version).catch(() => undefined);
          }
        };
        const detach = authorization.subscribe(authorizationChanged);
        authorizationChanged();
        return {
          ...authorization,
          dispose() {
            detach();
            authorization.dispose();
          },
        };
      },
    },
    configuration: {
      async configure(input, signal) {
        signal.throwIfAborted();
        const version = epoch;
        const reference = peer;
        if (reference === null || !current(version)) {
          throw new Error("Connect a node before configuring a provider.");
        }
        const account = providerAccount(
          await callRemote(reference, "configure", [input], unavailable),
        );
        signal.throwIfAborted();
        if (!current(version)) {
          throw new Error(
            "The node session changed while configuring the provider. Check its accounts before retrying.",
          );
        }
        const session = connection;
        if (session === null) {
          throw new Error("The node session ended while loading its catalog.");
        }
        await load(session.nodeId, version);
        if (!current(version)) {
          throw new Error("The provider result belongs to an earlier node session.");
        }
        return state.getSnapshot().accounts.find((entry) => entry.id === account.id) ?? account;
      },
    },
  };
  const detach = nodes.subscribe(changed);
  changed();
  return {
    backend,
    dispose() {
      closed = true;
      epoch += 1;
      authorizationScope.abort();
      detach();
      peer?.release();
      peer = null;
      connection = null;
    },
  };
}

export function createProvidersPresentation(
  state: UiStore<Readonly<ProvidersState>>,
  commit: (change: (current: Readonly<ProvidersState>) => ProvidersState) => Promise<unknown>,
  nodes: Snapshot<Readonly<ConnectionsState>>,
  connections: Connections,
  backend?: ProvidersBackend,
  nodeServices = false,
) {
  const node =
    backend === undefined && nodeServices
      ? createNodeProvidersBackend(state, nodes, connections)
      : null;
  const api = createProvidersApi(state, commit, backend ?? node?.backend);
  return {
    api,
    dispose() {
      node?.dispose();
      api.dispose();
    },
  };
}
