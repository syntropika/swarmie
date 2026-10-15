import { expect, test } from "bun:test";
import { setTimeout } from "node:timers/promises";

import type { Passkey } from "@swarmie/api";
import { QueryObserver } from "@tanstack/react-query";

import { passkeysQueryOptions } from "~/src/identity/passkeys/query";
import { clearAccountQueries } from "~/src/platform/account-cache";
import { createQueryClient } from "~/src/platform/query-client";
import { authEffects } from "~/src/test-session";

const savedKey: Passkey = {
  id: "saved-key",
  name: "Personal laptop",
  created_at: "2026-09-23T10:00:00Z",
  last_used_at: null,
};

test("returning to Account retains passkeys while refreshing their current state", async () => {
  const client = createQueryClient();
  const refresh = Promise.withResolvers<Passkey[]>();
  let reads = 0;
  const options = {
    ...passkeysQueryOptions("person", authEffects),
    queryFn: async () => {
      reads += 1;
      return reads === 1 ? [savedKey] : await refresh.promise;
    },
  };
  const first = new QueryObserver(client, options);
  let returning: typeof first | undefined;
  try {
    first.subscribe(() => {});
    await client.query(options);
    first.destroy();
    // Allow the default zero-duration garbage collection to run before revisiting the screen.
    await setTimeout(10);

    returning = new QueryObserver(client, options);
    returning.subscribe(() => {});
    expect(returning.getCurrentResult()).toMatchObject({
      data: [savedKey],
      isPending: false,
      isFetching: true,
    });
    expect(reads).toBe(2);

    refresh.resolve([]);
    await client.query(options);
    expect(returning.getCurrentResult().data).toEqual([]);
  } finally {
    refresh.resolve([]);
    first.destroy();
    returning?.destroy();
    client.clear();
  }
});

test("retained passkeys remain account-scoped and leave with account cleanup", async () => {
  const client = createQueryClient();
  const options = {
    ...passkeysQueryOptions("first-person", authEffects),
    queryFn: async () => [savedKey],
  };
  try {
    await client.query(options);
    expect(client.getQueryData<Passkey[]>(options.queryKey)).toEqual([savedKey]);
    expect(
      client.getQueryData<Passkey[]>(passkeysQueryOptions("second-person", authEffects).queryKey),
    ).toBeUndefined();

    await clearAccountQueries(client);
    expect(client.getQueryData<Passkey[]>(options.queryKey)).toBeUndefined();
  } finally {
    client.clear();
  }
});
