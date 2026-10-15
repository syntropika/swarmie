import { connectNativeClient } from "@swarmie/react-native";
import type { MobileClient } from "@swarmie/react-native";
import { QueryClient, QueryClientProvider, useMutation, focusManager } from "@tanstack/react-query";
import * as SecureStore from "expo-secure-store";
import { createContext, useContext, useEffect, useState, useMemo } from "react";
import type { ReactNode } from "react";
import { AppState } from "react-native";

import { connectionSchema } from "~/src/connection/settings";
import type { ConnectionSettings } from "~/src/connection/settings";

const credentialKey = "swarmie.runtime.connection";
type ConnectionError = "connection.error" | "connection.restoreError";
type Connection = { client: MobileClient; scope: string; resources: Set<string> };
type Context = {
  connection: Connection | null;
  restoring: boolean;
  error: ConnectionError | null;
  connect: (settings: ConnectionSettings) => void;
  disconnect: () => void;
  pending: boolean;
};
const ConnectionContext = createContext<Context | null>(null);

async function open(settings: ConnectionSettings): Promise<Connection> {
  const client = connectNativeClient(connectionSchema.parse(settings));
  try {
    const [runtime, extensions] = await Promise.all([
      client.call("runtime/initialize", {}),
      client.call("extension/list", {}),
    ]);
    const resource = extensions.find((extension) => extension.id === "resources");
    return {
      client,
      scope: runtime.runtimeIdentity.identity.storeId,
      resources: new Set(resource?.commands.map((command) => command.name)),
    };
  } catch (error) {
    client.dispose();
    throw error;
  }
}

function ConnectionOwner({ children, cache }: { children: ReactNode; cache: QueryClient }) {
  const [connection, setConnection] = useState<Connection | null>(null);
  const [restoring, setRestoring] = useState(true);
  const [restoreError, setRestoreError] = useState<ConnectionError | null>(null);
  const mutation = useMutation({
    mutationFn: async (settings: ConnectionSettings) => {
      const next = await open(settings);
      try {
        await SecureStore.setItemAsync(credentialKey, JSON.stringify(settings), {
          keychainAccessible: SecureStore.WHEN_UNLOCKED_THIS_DEVICE_ONLY,
        });
      } catch (error) {
        next.client.dispose();
        throw error;
      }
      return next;
    },
    onSuccess: (next) => {
      connection?.client.dispose();
      cache.clear();
      setRestoreError(null);
      setConnection(next);
    },
    retry: false,
  });
  const disconnect = useMutation({
    mutationFn: async () => {
      await SecureStore.deleteItemAsync(credentialKey);
    },
    onSuccess: () => {
      connection?.client.dispose();
      cache.clear();
      setConnection(null);
      mutation.reset();
    },
  });

  useEffect(() => {
    let active = true;
    let opened: Connection | undefined;
    async function restore() {
      try {
        const encoded = await SecureStore.getItemAsync(credentialKey);
        if (encoded !== null) {
          const value: unknown = JSON.parse(encoded);
          opened = await open(connectionSchema.parse(value));
          if (active) {
            setConnection(opened);
          } else {
            opened.client.dispose();
          }
        }
      } catch {
        if (active) {
          setRestoreError("connection.restoreError");
        }
      } finally {
        if (active) {
          setRestoring(false);
        }
      }
    }
    restore().catch(() => {
      if (active) {
        setRestoring(false);
      }
    });
    const listener = AppState.addEventListener("change", (state) => {
      focusManager.setFocused(state === "active");
    });
    return () => {
      active = false;
      listener.remove();
    };
  }, []);

  useEffect(
    () => () => {
      connection?.client.dispose();
    },
    [connection],
  );

  const value = useMemo<Context>(
    () => ({
      connection,
      restoring,
      error: mutation.isError || disconnect.isError ? "connection.error" : restoreError,
      connect: mutation.mutate,
      disconnect: () => {
        disconnect.mutate();
      },
      pending: mutation.isPending || disconnect.isPending,
    }),
    [connection, restoring, mutation, disconnect, restoreError],
  );
  return <ConnectionContext value={value}>{children}</ConnectionContext>;
}

export function RuntimeProvider({ children }: { children: ReactNode }) {
  const cache = useMemo(
    () =>
      new QueryClient({
        defaultOptions: { queries: { retry: 1, staleTime: 1000 }, mutations: { retry: false } },
      }),
    [],
  );
  return (
    <QueryClientProvider client={cache}>
      <ConnectionOwner cache={cache}>{children}</ConnectionOwner>
    </QueryClientProvider>
  );
}

export function useConnection() {
  const value = useContext(ConnectionContext);
  if (!value) {
    throw new Error("RuntimeProvider is required");
  }
  return value;
}

export function useRuntime() {
  const { connection } = useConnection();
  if (!connection) {
    throw new Error("Connect a runtime first");
  }
  return connection;
}
