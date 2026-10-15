import type { User } from "@swarmie/api";

/** A complete public session record for tests that exercise CSRF preparation. */
export const fixtureUser: User = {
  id: "85d45559-2944-479b-a3ce-28e5ea91bd8e",
  email: "member@example.test",
  email_verified: true,
  display_name: null,
  avatar_url: null,
  created_at: "2026-09-19T00:00:00Z",
  updated_at: "2026-09-19T00:00:00Z",
};

/** Capture a rejected operation without accepting an unexpected successful result. */
export async function rejectionOf(operation: Promise<unknown>): Promise<unknown> {
  try {
    await operation;
  } catch (error: unknown) {
    return error;
  }
  throw new Error("Expected the operation to reject");
}

/** Preserve the URL of each input accepted by the fetch contract in transport fixtures. */
export function requestUrl(input: RequestInfo | URL): string {
  return input instanceof Request ? input.url : input.toString();
}

/** Validate the JSON body boundary before inspecting the fixture's request payload. */
export function requestBody(init: RequestInit | undefined): unknown {
  if (typeof init?.body !== "string") {
    throw new TypeError("Expected a JSON string request body");
  }
  return JSON.parse(init.body);
}
