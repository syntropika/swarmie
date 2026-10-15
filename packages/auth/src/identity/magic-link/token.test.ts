import { describe, expect, test } from "bun:test";

import { takeMagicLinkToken } from "~/src/identity/magic-link/token";

const token = "a".repeat(43);

function capture(hash: string) {
  const replacements: { state: unknown; url: string | URL | null | undefined }[] = [];
  const state = { key: "router-entry" };
  const result = takeMagicLinkToken(
    { hash, pathname: "/auth/magic-link", search: "?source=email" },
    {
      state,
      replaceState: (nextState, _unused, url) => {
        replacements.push({ state: nextState, url });
      },
    },
  );
  return { result, replacements, state };
}

describe("magic-link token handoff", () => {
  test("returns a valid fragment token only in memory and scrubs the existing history entry", () => {
    const { result, replacements, state } = capture(`#token=${token}`);
    expect(result).toBe(token);
    expect(replacements).toEqual([{ state, url: "/auth/magic-link?source=email" }]);
  });

  test("also removes malformed fragments and rejects missing, ambiguous and invalid tokens", () => {
    for (const hash of [
      "#token=short",
      `#token=${token}&token=${token}`,
      `#token=${"a".repeat(44)}`,
      `#token=${"!".repeat(43)}`,
      "#unrelated=value",
    ]) {
      const { result, replacements } = capture(hash);
      expect(result).toBeNull();
      expect(replacements).toHaveLength(1);
      expect(String(replacements[0]?.url)).not.toContain("#");
    }
    expect(capture("").result).toBeNull();
    expect(capture("").replacements).toHaveLength(0);
  });

  test("a new hash can be taken after the first token has already been scrubbed", () => {
    const newer = "b".repeat(43);
    expect(capture(`#token=${token}`).result).toBe(token);
    expect(capture(`#token=${newer}`).result).toBe(newer);
  });
});
