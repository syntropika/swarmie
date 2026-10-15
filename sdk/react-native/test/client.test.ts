import { expect, test } from "bun:test";
import { rejects } from "node:assert/strict";

import { MobileClient } from "../src/client";
import type { NativeBinding } from "../src/client";

function fixture(result: unknown) {
  const commands: Record<string, unknown>[] = [];
  const binding: NativeBinding = {
    control(encoded) {
      const value: unknown = JSON.parse(encoded);
      if (typeof value !== "object" || value === null || Array.isArray(value)) {
        throw new Error("Invalid fixture command");
      }
      const command: Record<string, unknown> = Object.fromEntries(Object.entries(value));
      commands.push(command);
      return JSON.stringify({ ok: command.action === "create_http" ? 1 : 2 });
    },
    async execute(encoded) {
      const value: unknown = JSON.parse(encoded);
      if (typeof value !== "object" || value === null || Array.isArray(value)) {
        throw new Error("Invalid fixture command");
      }
      const command: Record<string, unknown> = Object.fromEntries(Object.entries(value));
      commands.push(command);
      if (commands.some((entry) => entry.action === "cancel")) {
        return JSON.stringify({ error: { kind: "cancelled" } });
      }
      return JSON.stringify({ ok: JSON.stringify(result) });
    },
  };
  return {
    commands,
    client: new MobileClient(binding, {
      url: "https://runtime.example.com",
      token: "device-token",
    }),
  };
}

test("native calls marshal the WIT request and release operation handles", async () => {
  const { client, commands } = fixture(null);
  expect(await client.call("session/close", { sessionId: "session-a" })).toBeNull();
  expect(commands[2]).toMatchObject({
    action: "invoke",
    client: 1,
    operation: 2,
    request: {
      method: "session/close",
      params_json: '{"sessionId":"session-a"}',
      timeout_ms: null,
    },
  });
  expect(commands.at(-1)).toEqual({ action: "release", handle: 2 });
  client.dispose();
  client.dispose();
  expect(
    commands.filter((command) => command.action === "release" && command.handle === 1),
  ).toHaveLength(1);
});

test("pre-dispatch cancellation crosses the native boundary and still releases ownership", async () => {
  const { client, commands } = fixture(null);
  const controller = new AbortController();
  controller.abort();
  await rejects(
    client.call("session/close", { sessionId: "session-a" }, controller.signal),
    /cancelled/u,
  );
  expect(commands.findIndex((command) => command.action === "cancel")).toBeLessThan(
    commands.findIndex((command) => command.action === "invoke"),
  );
  expect(commands.at(-1)).toEqual({ action: "release", handle: 2 });
  client.dispose();
});

test("method-specific schemas reject malformed runtime data", async () => {
  const { client } = fixture("invalid session list");
  await rejects(client.call("session/list", {}), /Invalid wire response/u);
  client.dispose();
});
