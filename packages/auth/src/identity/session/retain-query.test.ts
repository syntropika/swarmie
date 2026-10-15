import { afterAll, afterEach, expect, spyOn, test } from "bun:test";

import type { User } from "@swarmie/api";
import { QueryClient, QueryObserver } from "@tanstack/react-query";
import * as Effect from "effect/Effect";

import { retainSessionQuery } from "~/src/identity/session/retain-query";
import { clearAccountQueries } from "~/src/platform/account-cache";
import { authEffects, sessionQueryOptions } from "~/src/test-session";
import { fixtureUser } from "~/src/test-support";

const session = spyOn(authEffects, "session");

afterEach(() => {
  session.mockReset();
});
afterAll(() => {
  session.mockRestore();
});

test("public entry retains no identity and never starts a session read", () => {
  const client = new QueryClient();
  const release = retainSessionQuery(client, sessionQueryOptions);
  try {
    expect(session).not.toHaveBeenCalled();
    expect(client.getQueryState(sessionQueryOptions.queryKey)).toBeUndefined();
  } finally {
    release();
    client.clear();
  }
});

test("a pending session refresh is shared across a gap between Account and Workspace observers", async () => {
  const client = new QueryClient();
  client.setQueryData(sessionQueryOptions.queryKey, fixtureUser);
  const pending = Promise.withResolvers<User>();
  const started = Promise.withResolvers<undefined>();
  let cancellations = 0;
  session.mockReturnValue(
    Effect.promise(async () => {
      started.resolve();
      return await pending.promise;
    }).pipe(
      Effect.onInterrupt(() =>
        Effect.sync(() => {
          cancellations += 1;
        }),
      ),
    ),
  );
  const release = retainSessionQuery(client, sessionQueryOptions);
  const first = new QueryObserver(client, { ...sessionQueryOptions, staleTime: 0 });
  const leaveFirst = first.subscribe(() => undefined);
  let leaveSecond: (() => void) | undefined;
  try {
    await started.promise;
    leaveFirst();
    expect(client.getQueryState(sessionQueryOptions.queryKey)?.fetchStatus).toBe("fetching");
    const second = new QueryObserver(client, { ...sessionQueryOptions, staleTime: 0 });
    leaveSecond = second.subscribe(() => undefined);
    const refreshing = client.query({ ...sessionQueryOptions, staleTime: 0 });
    pending.resolve(fixtureUser);
    await refreshing;
    expect(session).toHaveBeenCalledTimes(1);
    expect(cancellations).toBe(0);
  } finally {
    pending.resolve(fixtureUser);
    leaveFirst();
    leaveSecond?.();
    release();
    client.clear();
  }
});

test("account cleanup still cancels retained reads and later login can establish a new lifetime", async () => {
  const client = new QueryClient();
  const release = retainSessionQuery(client, sessionQueryOptions);
  let cancellations = 0;
  session.mockReturnValue(
    Effect.never.pipe(
      Effect.onInterrupt(() =>
        Effect.sync(() => {
          cancellations += 1;
        }),
      ),
    ),
  );
  let leave: (() => void) | undefined;
  async function admitThenClear(id: string) {
    client.setQueryData(sessionQueryOptions.queryKey, { ...fixtureUser, id });
    const observer = new QueryObserver(client, { ...sessionQueryOptions, staleTime: 0 });
    leave = observer.subscribe(() => undefined);
    leave();
    expect(client.getQueryState(sessionQueryOptions.queryKey)?.fetchStatus).toBe("fetching");
    await clearAccountQueries(client);
    expect(client.getQueryState(sessionQueryOptions.queryKey)).toBeUndefined();
  }
  try {
    await admitThenClear("first-account");
    await admitThenClear("second-account");
    expect(session).toHaveBeenCalledTimes(2);
    expect(cancellations).toBe(2);
  } finally {
    leave?.();
    release();
    client.clear();
  }
});
