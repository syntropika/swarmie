import assert from "node:assert/strict";
import { once } from "node:events";
import { mkdtemp, rm } from "node:fs/promises";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { setTimeout as sleep } from "node:timers/promises";
import { promisify } from "node:util";

import { Client as BrowserClient } from "../dist/browser.js";
import { Client, RpcError, ProtocolError, decodeResponse } from "../dist/client.js";
import { stdioClient } from "../dist/stdio.js";

test("decodes null results and preserves structured errors", () => {
  assert.equal(decodeResponse({ jsonrpc: "2.0", id: "a", result: null }, "a"), null);
  assert.throws(() => decodeResponse({ jsonrpc: "2.0", id: "b", result: {} }, "a"), ProtocolError);
  assert.throws(
    () =>
      decodeResponse(
        {
          jsonrpc: "2.0",
          id: "a",
          error: { code: -32_602, message: "invalid", data: { field: "cwd" } },
        },
        "a",
      ),
    // oxlint-disable-next-line promise/prefer-await-to-callbacks -- This is a synchronous exception predicate, not an asynchronous callback.
    (error) => error instanceof RpcError && error.code === -32_602 && error.data.field === "cwd",
  );
});
for (const Implementation of [Client, BrowserClient]) {
  test(`${Implementation === Client ? "native" : "browser"} HTTP enforces identity, byte limit and cancellation`, async () => {
    const server = createServer(async (req, res) => {
      let body = "";
      for await (const chunk of req) {
        body += chunk;
      }
      const request = JSON.parse(body);
      if (request.method === "slow") {
        setTimeout(() => res.end("{}"), 100);
        return;
      }
      res.setHeader("content-type", "application/json");
      res.end(
        JSON.stringify({
          jsonrpc: "2.0",
          id: request.method === "wrong" ? "wrong" : request.id,
          result: request.method === "large" ? "x".repeat(2048) : null,
        }),
      );
    });
    server.listen(0, "127.0.0.1");
    await once(server, "listening");
    const url = `http://127.0.0.1:${server.address().port}`;
    try {
      const client = Implementation.http(url, { timeoutMs: 1000, maxResponseBytes: 1024 });
      assert.equal(await client.callRaw("null"), null);
      await assert.rejects(client.callRaw("wrong"), ProtocolError);
      await assert.rejects(client.callRaw("large"), ProtocolError);
      await assert.rejects(Implementation.http(url, { timeoutMs: 20 }).callRaw("slow"));
    } finally {
      server.closeAllConnections();
      await promisify(server.close.bind(server))();
    }
  });
}

test("event polling rejects unordered transcript and has a deadline", async () => {
  const server = createServer(async (req, res) => {
    let body = "";
    for await (const chunk of req) {
      body += chunk;
    }
    const request = JSON.parse(body);
    res.end(
      JSON.stringify({
        jsonrpc: "2.0",
        id: request.id,
        result:
          request.params.sessionId === "unordered"
            ? [
                { sessionId: "unordered", sequence: 2, type: "text.delta", data: { text: "a" } },
                { sessionId: "unordered", sequence: 1, type: "text.delta", data: { text: "b" } },
              ]
            : [],
      }),
    );
  });
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  const client = Client.http(`http://127.0.0.1:${server.address().port}`);
  try {
    await assert.rejects(async () => {
      for await (const event of client.events("unordered")) {
        assert.ok(event);
      }
    }, ProtocolError);
    await assert.rejects(
      client.waitForTurn("s", { timeoutMs: 20, pollIntervalMs: 2 }),
      /deadline/u,
    );
  } finally {
    client.dispose();
    server.closeAllConnections();
    await promisify(server.close.bind(server))();
  }
});
async function conformance(client) {
  const initialized = await client.initialize();
  assert.ok(initialized);
  const schema = await client.schema();
  assert.ok(Object.keys(schema["x-methods"]).includes("session/create"));
  const harnesses = await client.discover();
  assert.ok(harnesses.some((harness) => harness.harness === "mock" && harness.available));
  await assert.rejects(
    client.callRaw("not/a/method"),
    (error) => error instanceof RpcError && error.code === -32_601,
  );
  const session = await client.spawn({
    harness: "mock",
    cwd: process.env.SWARMIE_TEST_CWD ?? process.cwd(),
  });
  try {
    assert.equal(session.capabilities.queue.source, "runtime");
    assert.equal(session.capabilities.configure.availability, "available");
    const models = await client.call("agent/models", { sessionId: session.id });
    assert.equal(models[0].metadata.nativeField, "retained");
    const settings = await client.call("agent/configuration", { sessionId: session.id });
    assert.equal(settings[0].application, "next_turn");
    const configured = await client.call("agent/configure", {
      sessionId: session.id,
      config: { fast: true },
    });
    assert.equal(configured.values.fast, true);
    assert.equal(configured.configuration[0].value, true);
    assert.equal(settings.at(-1).id, "executionProfile");
    assert.equal(configured.executionPolicy.status, "unknown");
    assert.equal(configured.executionPolicy.dispatchBlocked, false);
    assert.equal(configured.executionPolicy.choices.length, 3);
    const accepted = await client.prompt(
      session.id,
      [{ type: "text", data: "Reply" }],
      "request-1",
    );
    assert.equal(accepted.delivery, "accepted");
    const events = await client.waitForTurn(session.id, {
      afterSequence: session.sequence,
      timeoutMs: 3000,
      pollIntervalMs: 5,
    });
    assert.equal(
      events
        .filter((event) => event.type === "text.delta")
        .map((event) => event.data.text)
        .join(""),
      "client-ready",
    );
    assert.equal(events.find((event) => event.type === "turn.ended")?.data.outcome, "completed");
    assert.ok(events.every((e, i) => i === 0 || e.sequence > events[i - 1].sequence));
    const recovered = await client.submission(session.id, "request-1");
    assert.equal(recovered.delivery, "accepted");
    assert.equal(recovered.outcome, "completed");
    const reconciled = await client.reconcileSubmission(session.id, "request-1");
    assert.equal(reconciled.id, recovered.id);
    const duplicate = await client.prompt(session.id, "Reply", "request-1");
    assert.equal(duplicate.id, recovered.id);
    await assert.rejects(
      client.prompt(session.id, "Changed", "request-1"),
      (error) =>
        error instanceof RpcError &&
        error.code === -32_004 &&
        error.data.code === "idempotency_conflict",
    );
    const replay = await client.call("session/transcript", { sessionId: session.id, limit: 4096 });
    assert.ok(replay.some((e) => e.type === "text.delta"));
    assert.equal(replay.filter((e) => e.type === "turn.started").length, 1);
  } finally {
    await client.close(session.id);
  }
}
test("live HTTP daemon conformance", { skip: !process.env.SWARMIE_TEST_URL }, async () => {
  await conformance(
    Client.http(process.env.SWARMIE_TEST_URL, { token: process.env.SWARMIE_TEST_TOKEN }),
  );
  await assert.rejects(
    Client.http(process.env.SWARMIE_TEST_URL, { token: "incorrect" }).initialize(),
    ProtocolError,
  );
});
test(
  "live owned stdio daemon conformance",
  { skip: !process.env.SWARMIE_TEST_BINARY },
  async () => {
    const state = await mkdtemp(path.join(tmpdir(), "swarmie-ts-client-"));
    const owned = stdioClient(process.env.SWARMIE_TEST_BINARY, [
      "--state-dir",
      state,
      "serve",
      "--mock-script",
      process.env.SWARMIE_TEST_SCRIPT,
    ]);
    try {
      await conformance(owned.client);
    } finally {
      owned.close();
      await rm(state, { recursive: true, force: true });
    }
  },
);
test("owned stdio deadlines terminate a nonresponsive process", async () => {
  const owned = stdioClient(process.execPath, ["-e", "process.stdin.resume()"], 20);
  try {
    await assert.rejects(owned.client.initialize(), /timed out/u);
    await assert.rejects(owned.client.initialize(), /closed/u);
  } finally {
    owned.close();
  }
});

test("native HTTP supports concurrent requests and abort before or during dispatch", async () => {
  const identities = new Set();
  const server = createServer(async (req, res) => {
    let body = "";
    for await (const chunk of req) {
      body += chunk;
    }
    const request = JSON.parse(body);
    assert.equal(identities.has(request.id), false);
    identities.add(request.id);
    if (request.method === "slow") {
      await sleep(100);
    }
    res.end(JSON.stringify({ jsonrpc: "2.0", id: request.id, result: request.method }));
  });
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  const client = Client.http(`http://127.0.0.1:${server.address().port}`);
  try {
    const results = await Promise.all(
      Array.from({ length: 20 }, () => client.callRaw("concurrent")),
    );
    assert.equal(results.length, 20);
    const before = new AbortController();
    before.abort();
    await assert.rejects(client.callRaw("slow", {}, before.signal), { name: "AbortError" });
    const during = new AbortController();
    const pending = client.callRaw("slow", {}, during.signal);
    setTimeout(() => during.abort(), 10);
    await assert.rejects(pending, { name: "AbortError" });
  } finally {
    client.dispose();
    server.closeAllConnections();
    await promisify(server.close.bind(server))();
  }
});
