/** Complete non-secret session identity used by transport boundary tests. */
export const userFixture = {
  id: "user-fixture",
  email: "member@example.test",
  email_verified: true,
  display_name: null,
  avatar_url: null,
  created_at: "2026-09-23T00:00:00Z",
  updated_at: "2026-09-23T00:00:00Z",
};

export const workspaceFixture = {
  id: "workspace",
  name: "Team",
  created_by_user_id: userFixture.id,
  created_at: "2026-09-23T00:00:00Z",
  updated_at: "2026-09-23T00:00:00Z",
  deleted_at: null,
};

export function requestUrl(input: RequestInfo | URL): string {
  return input instanceof Request ? input.url : input.toString();
}

export function requestBody(body: BodyInit | null | undefined): unknown {
  if (typeof body !== "string") {
    throw new TypeError("Expected a serialized JSON request body");
  }
  const result: unknown = JSON.parse(body);
  return result;
}

/** Await rejection explicitly because Bun's matcher types declare asynchronous matchers as void. */
export async function rejected(operation: Promise<unknown>): Promise<unknown> {
  const [result] = await Promise.allSettled([operation]);
  if (result?.status !== "rejected") {
    throw new Error("Expected the operation to reject");
  }
  return result.reason;
}
