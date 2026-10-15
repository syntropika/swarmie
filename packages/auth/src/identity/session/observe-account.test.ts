import { expect, test } from "bun:test";

import { QueryClient } from "@tanstack/react-query";

import { observeSessionAccount } from "~/src/identity/session/observe-account";
import { readCachedSession } from "~/src/identity/session/resolve";
import { registerAccountResource } from "~/src/platform/account-cache";
import { sessionQueryOptions } from "~/src/test-session";
import { fixtureUser } from "~/src/test-support";

test("background account replacement clears old resources before requesting fresh admission", async () => {
  const client = new QueryClient({ defaultOptions: { queries: { gcTime: Infinity } } });
  client.setQueryData(sessionQueryOptions.queryKey, fixtureUser);
  client.setQueryData(["account", "workspace", fixtureUser.id], { name: "Old workspace" });
  const completed = Promise.withResolvers<undefined>();
  let cleaned = 0;
  const unregister = registerAccountResource(client, () => {
    cleaned += 1;
  });
  const stop = observeSessionAccount(client, fixtureUser.id, async () => {
    expect(cleaned).toBe(1);
    expect(readCachedSession(client)).toBeUndefined();
    expect(
      client.getQueryData<{ name: string }>(["account", "workspace", fixtureUser.id]),
    ).toBeUndefined();
    completed.resolve();
  });
  try {
    client.setQueryData(sessionQueryOptions.queryKey, { ...fixtureUser, id: "another-account" });
    await completed.promise;
    expect(cleaned).toBe(1);
  } finally {
    stop();
    unregister();
    client.clear();
  }
});

test("same-account refresh preserves admitted state and an unmounted observer cannot redirect", async () => {
  const client = new QueryClient({ defaultOptions: { queries: { gcTime: Infinity } } });
  client.setQueryData(sessionQueryOptions.queryKey, fixtureUser);
  let redirects = 0;
  const cancelled = Promise.withResolvers<undefined>();
  const cancel = spyCancel(client, cancelled.promise);
  const stop = observeSessionAccount(client, fixtureUser.id, async () => {
    redirects += 1;
  });
  try {
    client.setQueryData(sessionQueryOptions.queryKey, {
      ...fixtureUser,
      display_name: "Updated name",
    });
    expect(readCachedSession(client)?.display_name).toBe("Updated name");
    expect(redirects).toBe(0);
    client.setQueryData(sessionQueryOptions.queryKey, { ...fixtureUser, id: "another-account" });
    stop();
    cancelled.resolve();
    await Promise.resolve();
    await Promise.resolve();
    expect(redirects).toBe(0);
  } finally {
    stop();
    cancelled.resolve();
    cancel();
    client.clear();
  }
});

/** Hold cleanup to expose the unmount race without timers or a mocked React lifecycle. */
function spyCancel(client: QueryClient, gate: Promise<undefined>): () => void {
  const cancel = client.cancelQueries.bind(client);
  client.cancelQueries = async (...args) => {
    await cancel(...args);
    await gate;
  };
  return () => {
    client.cancelQueries = cancel;
  };
}
