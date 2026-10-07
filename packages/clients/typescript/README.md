# TypeScript runtime client

Install a local archive produced by `tools/package_local.py`, or build this directory:

```sh
npm ci
npm run build
```

```ts
import { Client } from "@mpai/client";

const client = Client.http("http://127.0.0.1:7457", { token: process.env.MPAI_TOKEN });
await client.initialize();
const session = await client.spawn({ harness: "codex", cwd: "/absolute/project" });
try {
  await client.prompt(session.id, "Describe the project.");
  for await (const event of client.events(session.id, { afterSequence: session.sequence })) {
    if (event.type === "text.delta") process.stdout.write(event.data.text);
    if (event.type === "permission.requested") {
      // Present the request and submit the user's chosen option with permission/respond.
    }
    if (event.type === "turn.ended") break;
  }
} finally {
  await client.close(session.id);
}
```

Use `call(method, params)` for all generated RPC methods or `callRaw` for an unknown future method. `RpcError` preserves `code` and `data`. Pass an `AbortSignal` to `call` or `events` for cancellation. Event polling defaults to ten minutes, 200 ms intervals, and 256 events per page; requests default to 30 seconds and 16 MiB responses.

For an owned process in Node:

```ts
import { stdioClient } from "@mpai/client/stdio";
const owned = stdioClient("/absolute/path/mpai", ["serve"]);
try { await owned.client.initialize(); } finally { owned.close(); }
```

A stdio timeout or abort closes the process and rejects other pending calls. HTTP is usable in a browser when the daemon's origin policy allows that origin. HTTP and stdio are supported; WebSocket and Unix socket are not provided by this package. No registry publication is implied by the package name.
