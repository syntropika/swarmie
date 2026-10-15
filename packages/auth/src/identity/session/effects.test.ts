import { expect, test } from "bun:test";

import { SwarmieClient } from "@swarmie/api";

import { createAuthEffects, runAuthEffect } from "~/src/identity/authentication/effects";
import { fixtureUser, rejectionOf, requestUrl } from "~/src/test-support";

/** Exercise the real cookie-aware client with a local response fixture and no real credentials. */
function fixture(logoutStatus: number, sessionStatus: number) {
  const calls: string[] = [];
  let loggingOut = false;
  const client = new SwarmieClient({
    fetch: async (input) => {
      const path = requestUrl(input);
      calls.push(path);
      if (path === "/auth/logout") {
        loggingOut = true;
        return new Response(null, { status: logoutStatus });
      }
      const status = loggingOut ? sessionStatus : 200;
      return Response.json({ user: fixtureUser, csrf_token: "fixture" }, { status });
    },
  });
  return {
    calls,
    logout: async () => {
      await runAuthEffect(createAuthEffects(client).logout());
    },
  };
}

test("logout confirms local cookie removal after upstream revocation fails", async () => {
  const api = fixture(503, 401);
  await api.logout();
  expect(api.calls).toEqual(["/api/me", "/auth/logout", "/api/me"]);
});

test("a still-active session preserves the logout failure without retrying the mutation", async () => {
  const api = fixture(503, 200);
  expect(await rejectionOf(api.logout())).toMatchObject({ _tag: "RequestError", status: 503 });
  expect(api.calls.filter((path) => path === "/auth/logout")).toHaveLength(1);
});

test("an unavailable session check does not pretend that logout completed", async () => {
  const api = fixture(503, 502);
  expect(await rejectionOf(api.logout())).toMatchObject({ _tag: "RequestError", status: 502 });
});

test("successful logout requires no additional authenticated read", async () => {
  const api = fixture(204, 200);
  await api.logout();
  expect(api.calls).toEqual(["/api/me", "/auth/logout"]);
});
