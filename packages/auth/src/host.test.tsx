import { expect, test } from "bun:test";

import { SwarmieClient } from "@swarmie/api";
import type { User } from "@swarmie/api";
import { QueryClient } from "@tanstack/react-query";
import * as Effect from "effect/Effect";
import { renderToStaticMarkup } from "react-dom/server";

import { AuthenticationProvider, Link } from "~/src/host";
import type { AuthenticationHost } from "~/src/host";
import { createAuthEffects } from "~/src/identity/authentication/effects";
import { LoginController } from "~/src/identity/login";
import { createSessionQueryOptions } from "~/src/identity/session/query";
import { requestUrl, fixtureUser } from "~/src/test-support";

test("authentication presentation needs an explicit host and starts no account request during rendering", () => {
  let reads = 0;
  const host: AuthenticationHost = {
    client: new SwarmieClient({
      baseUrl: "https://accounts.example.test",
      fetch: () => {
        reads += 1;
        throw new Error("Unexpected render-time request");
      },
    }),
    queryClient: new QueryClient(),
    siteUrl: "https://www.example.test",
    navigate: async () => {
      await Promise.resolve();
    },
    onError: () => {},
  };
  try {
    const markup = renderToStaticMarkup(
      <AuthenticationProvider host={host}>
        <LoginController />
        <Link to="/signup">Create an account</Link>
      </AuthenticationProvider>,
    );
    expect(markup).toContain('href="/signup"');
    expect(markup).toContain('href="https://www.example.test"');
    expect(reads).toBe(0);
    expect(() => renderToStaticMarkup(<LoginController />)).toThrow(
      "AuthenticationProvider is required",
    );
  } finally {
    host.queryClient.clear();
  }
});

test("separate account hosts do not share transport, CSRF state, or session caches", async () => {
  const owners = ["first", "second"].map((owner) => {
    const requests: string[] = [];
    const client = new SwarmieClient({
      baseUrl: `https://${owner}.example.test`,
      fetch: async (input) => {
        requests.push(requestUrl(input));
        return await Promise.resolve(
          Response.json({ user: { ...fixtureUser, display_name: owner }, csrf_token: owner }),
        );
      },
    });
    return {
      owner,
      requests,
      effects: createAuthEffects(client),
      query: new QueryClient(),
    };
  });
  try {
    for (const host of owners) {
      const options = createSessionQueryOptions(host.effects);
      // oxlint-disable-next-line no-await-in-loop -- Exercise each independently owned session in order.
      const user = await host.query.query(options);
      expect(user.display_name).toBe(host.owner);
      expect(host.requests).toEqual([`https://${host.owner}.example.test/api/me`]);
    }
    const [first, second] = owners;
    if (!first || !second) {
      throw new Error("Both fixtures are required");
    }
    await Effect.runPromise(first.effects.logout());
    expect(first.requests).toHaveLength(2);
    expect(second.requests).toHaveLength(1);
    expect(
      second.query.getQueryData<User>(createSessionQueryOptions(second.effects).queryKey),
    ).toEqual({
      ...fixtureUser,
      display_name: second.owner,
    });
  } finally {
    for (const host of owners) {
      host.query.clear();
    }
  }
});
