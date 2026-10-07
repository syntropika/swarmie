# Runtime clients

Local TypeScript, Python, Go, and Swift clients share the daemon's JSON-RPC API. Each client supports HTTP, bearer authentication, typed session creation, text or image content, structured RPC errors, and ordered durable events. TypeScript and Python additionally own a stdio daemon process.

The clients do not launch or authenticate a provider when connecting over HTTP. Start the runtime and install the harness you want to use independently:

```sh
cargo build -p app --release
./target/release/mpai serve --http 127.0.0.1:7457
```

Set a bearer token when binding beyond loopback. Supply it through client options. Execution environment variables and MCP headers belong to input options, are never logged by these clients, and are absent from generated snapshot output types. Clients keep no credential files.

## Shared contracts

`schema/protocol.json` comes from `runtime/schema`, whose definitions are derived from the Rust domain. Refresh after protocol changes:

```sh
MPAI_TOKEN="$YOUR_LOCAL_TOKEN" python3 packages/clients/tools/generate.py --url http://127.0.0.1:7457
python3 packages/clients/tools/generate.py --check
```

The generator requires Python 3.11+ and `gofmt`. TypeScript has generated named parameters and results for every public method, including discriminated event variants. Python has generated `TypedDict` domain objects. Go and Swift have generated domain objects and method catalogs. Heterogeneous provider payloads and variant unions remain flexible JSON in Python, Go, and Swift; the canonical schema describes their full validation rules. Raw provider event data is preserved.

Call the generic `call` API for configuration, permissions, queues, extensions, forks, rollback, and methods without convenience helpers. Check session capabilities before invoking optional operations.

The catalog includes sessionless `runtime/quotas` and `runtime/quotas/all`. Typed domain definitions include questions/answers, plan entries, child relationships, file locations/diffs, and tool progress. Existing JSON metadata remains available alongside stable fields. Question responses may use top-level `answers`; account execution credentials remain input-only.

Event iterators poll the durable `session/transcript` with pages of 256 and a default 200 ms interval. They verify session identity and strictly increasing sequence numbers. For an existing session, pass the sequence captured **before** submitting a prompt to wait only for that new turn. A default ten-minute polling deadline and request timeout prevent indefinite waits. Breaking iteration stops polling; use cancellation where supported. `waitForTurn` collects one turn, while event iterators process pages with backpressure.

## Supported transports

| Client | HTTP | Owned stdio | WebSocket | Unix socket |
| --- | --- | --- | --- | --- |
| TypeScript, Node 22+ | Yes | Yes | No | No |
| Python 3.11+ | Yes | Yes, Unix | No | No |
| Go 1.22+ | Yes | No | No | No |
| Swift 6, macOS 13+/iOS 16+ | Yes | No | No | No |

HTTP polling is the maintained event path in these clients. The runtime's WebSocket transport remains available to independent clients; the runtime does not currently implement a Unix socket listener. HTTP response bodies are bounded to 16 MiB by default. Stdio frames are bounded to 16 MiB; a stdio timeout closes the owned process so a late response cannot be mistaken for another request. TypeScript permits concurrent stdio requests; Python serializes them and bounds lock and pipe waits. Go uses contexts, TypeScript uses `AbortSignal`, Swift uses task cancellation, and Python accepts a cancellation event between transcript requests. Python HTTP calls remain bounded by the request deadline while reading response chunks.

No npm, PyPI, Go proxy, or Swift registry release is claimed. Install from these directories or the local artifacts produced below.

## Local distribution

```sh
python3 packages/clients/tools/package_local.py --binary target/release/mpai
```

This builds an npm tarball and Python wheel, packages Go and Swift source modules, exports the canonical schema, and bundles the supplied runtime executable with its license. Output defaults to the workspace's external artifact directory. The runtime archive is for the current host platform and architecture; the script refuses an incompatible executable. It neither builds cross-platform binaries nor publishes packages. Python packaging requires installed `pip`, `setuptools>=68`, and `wheel`; TypeScript packaging requires Node and npm.

Use `cargo build -p app --release` first for a production executable. A debug executable can be explicitly supplied for local verification and is labeled as a local build. Separate macOS binaries must be built on macOS; code signing, notarization, installer creation, and registry publication remain release tasks.

## Verification

```sh
python3 scripts/client-conformance.py --swift-host macos-build-host
```

The runner starts a disposable actual daemon with `tests/fixtures/client-script.json`, checks generated contracts against it, and runs each client's HTTP lifecycle tests. Node and Python additionally run actual owned stdio sessions. Swift tests run through SSH on the requested macOS host with a loopback remote forward. Unit checks cover response identity, structured errors, limits, deadlines, and input-only credentials. Build and verification state stays outside the repository.
