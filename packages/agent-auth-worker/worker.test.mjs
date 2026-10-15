import assert from "node:assert/strict";
import test from "node:test";

import { performLogin } from "./worker.mjs";

test("Cursor uses the public SDK browser callback and never writes its default credential store", async () => {
  const events = [];
  const { signal } = new AbortController();
  const result = await performLogin(
    { harness: "cursor" },
    (event) => events.push(event),
    signal,
    () => undefined,
    {
      cursor: {
        Cursor: {
          auth: {
            login: async (options) => {
              assert.equal(options.store, null);
              assert.equal(options.openBrowser, false);
              assert.equal(options.signal, signal);
              options.onLoginUrl("https://cursor.com/login?challenge=fixture");
              return { apiKey: "private-minted-key", apiKeyExpiresAtMs: 123 };
            },
          },
        },
      },
    },
  );
  assert.equal(result.apiKey, "private-minted-key");
  assert.deepEqual(events, [
    { type: "authorization", url: "https://cursor.com/login?challenge=fixture" },
  ]);
  assert.ok(!JSON.stringify(events).includes("private-minted-key"));
});

test("Pi delegates OAuth prompts, device codes and cancellation to its native SDK", async () => {
  const events = [];
  const { signal } = new AbortController();
  const result = await performLogin(
    { harness: "pi", provider: "github-copilot" },
    (event) => events.push(event),
    signal,
    async () => "github.com",
    {
      piGitHub: {
        githubCopilotProvider: () => ({
          auth: {
            oauth: {
              login: async (interaction) => {
                assert.equal(interaction.signal, signal);
                assert.equal(
                  await interaction.prompt({ type: "text", message: "Domain" }),
                  "github.com",
                );
                interaction.notify({
                  type: "device_code",
                  userCode: "ABCD-EFGH",
                  verificationUri: "https://github.com/login/device",
                });
                return {
                  type: "oauth",
                  access: "private-access",
                  refresh: "private-refresh",
                  expires: 123,
                };
              },
            },
          },
        }),
      },
    },
  );
  assert.equal(result.nativeCredential.access, "private-access");
  assert.equal(events[0].userCode, "ABCD-EFGH");
  assert.ok(!JSON.stringify(events).includes("private-access"));
});

test("the pinned SDK modules expose authentication without starting an agent", async () => {
  const { Cursor } = await import("@cursor/sdk");
  const { openaiCodexProvider } = await import("@earendil-works/pi-ai/providers/openai-codex");
  const { githubCopilotProvider } = await import("@earendil-works/pi-ai/providers/github-copilot");
  assert.equal(typeof Cursor.auth.login, "function");
  assert.equal(typeof openaiCodexProvider().auth.oauth.login, "function");
  assert.equal(typeof githubCopilotProvider().auth.oauth.login, "function");
});
