# TypeScript runtime client

Install both the contracts and client archives produced by `sdk/tools/package_local.py`,
or build from the repository root:

```sh
bun install --frozen-lockfile
just sdk-build
bun run --cwd sdk/typescript build
```

The SDK depends on the public `@swarmie/contracts` package. Local source builds resolve it from `packages/contracts`; packaged SDKs use its exact package version. Generated runtime types are available directly from the contracts package.

```ts
import { Client } from "@swarmie/client";

const client = Client.http("http://127.0.0.1:7457", { token: process.env.SWARMIE_TOKEN });
await client.initialize();
const session = await client.spawn({ harness: "codex", cwd: "/absolute/project" });
try {
  const key = crypto.randomUUID(); // Persist this key before sending.
  await client.prompt(session.id, "Describe the project.", key);
  for await (const event of client.events(session.id, { afterSequence: session.sequence })) {
    if (event.type === "text.delta") process.stdout.write(event.data.text);
    if (event.type === "permission.requested") {
      // Present the request and submit the user's chosen option with permission/respond.
    }
    if (event.type === "turn.ended") break;
  }
} finally {
  await client.close(session.id);
  client.dispose();
}
```

Use `call(method, params)` for all generated RPC methods or `callRaw` for an unknown future method. `RpcError` preserves `code` and `data`. Pass an `AbortSignal` to `call` or `events` for cancellation. Event polling defaults to ten minutes, 200 ms intervals, and 256 events per page; requests default to 30 seconds and 16 MiB responses.

For an owned process in Node:

```ts
import { stdioClient } from "@swarmie/client/stdio";
const owned = stdioClient("/absolute/path/swarmie", ["serve"]);
try { await owned.client.initialize(); } finally { owned.close(); }
```

A stdio timeout or abort closes the process and rejects other pending calls. Node uses Node-API and the common Rust engine. Import `@swarmie/client/browser` explicitly in browser-only builds, or use the package browser export condition. That entry has no native module dependency. HTTP is usable in a browser when the daemon's origin policy allows that origin. HTTP and stdio are supported; WebSocket and Unix socket are not provided by this package. No registry publication is implied by the package name.

Each intentional prompt requires its own persisted idempotency key. Reuse the original key and content to obtain the retained receipt without dispatching again. Query `session/submission` after a lost response; this read does not open a native session. `delivery` distinguishes `not_sent`, `in_flight`, `accepted`, and `unknown`. Acceptance does not mean completion. An uncertain receipt blocks new work; clients do not automatically resend. Keys are scoped to a personal node session and survive event-history trimming while its submission ledger is retained.

Use `session/submission/reconcile` (or the client reconciliation helper) to check an already open native connection without resending work. Explicitly resume a closed session first. Native absence is not rejection; partial evidence and unsupported adapters keep uncertain work paused. Settled receipts require no native lookup. Check `capabilities.submissionReconciliation` for qualified lookup support.
