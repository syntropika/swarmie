# Runtime clients

Local TypeScript, Python, Go, and Swift clients share the daemon's JSON-RPC API. Each client supports HTTP, bearer authentication, typed session creation, text or image content, structured RPC errors, and ordered durable events. All four native SDKs can own a stdio daemon process on desktop hosts. Node, Python, Go, and Swift call one Rust client engine through native bindings. The browser export retains a portable HTTP implementation.

The clients do not launch or authenticate a provider when connecting over HTTP. Start the runtime and install the harness you want to use independently:

```sh
cargo build -p app --release
./target/release/swarmie serve --http 127.0.0.1:7457
```

Set a bearer token when binding beyond loopback. Supply it through client options. Execution environment variables and MCP headers belong to input options, are never logged by these clients, and are absent from generated snapshot output types. Clients keep no credential files.

## Native client boundary

`sdk/wit/client.wit` defines `swarmie:client/native@0.1.0`. The Rust build parses WIT and generates boundary records, enums, operation arguments, dispatch, and the implementation trait. Changing a WIT signature requires updating the Rust implementation. WIT describes this interface; native execution does not require a WebAssembly component or runtime.

`crates/client` owns HTTP, stdio, JSON-RPC validation, deadlines, cancellation, event ordering, and turn collection. `crates/client-node` exposes Node-API. `crates/client-ffi` exposes the C ABI used by Python ctypes, Go cgo, and Swift. Bindings marshal values, adapt language cancellation, and release handles. Protocol DTOs remain generated from the runtime schema; JSON strings cross the WIT boundary without a second set of domain definitions. See [native boundary and ownership](native-client.md).

Build from the parent root before importing native source packages:

```sh
just sdk-build
# On a macOS build host, include the static macOS/iOS XCFramework:
just sdk-build-apple
```

## Shared contracts

`packages/contracts/protocol.json` comes from `runtime/schema`, whose definitions are derived from the Rust domain. Refresh after protocol changes:

```sh
SWARMIE_TOKEN="$YOUR_LOCAL_TOKEN" python3 sdk/tools/generate.py --url http://127.0.0.1:7457
python3 sdk/tools/generate.py --check
```

The generator requires Python 3.11+ and `gofmt`. The standalone `@swarmie/contracts` package owns the JSON Schema and generated TypeScript types. The desktop uses these definitions directly through native IPC without importing the transport SDK. TypeScript has generated named parameters and results for every public method, including discriminated event variants. Python has generated `TypedDict` domain objects. Go and Swift have generated domain objects and method catalogs. Heterogeneous provider payloads and variant unions remain flexible JSON in Python, Go, and Swift; the canonical schema describes their full validation rules. Raw provider event data is preserved.

Call the generic `call` API for configuration, permissions, queues, extensions, forks, rollback, and methods without convenience helpers. Check session capabilities before invoking optional operations.

The catalog includes sessionless `runtime/quotas` and `runtime/quotas/all`. Typed domain definitions include questions/answers, plan entries, child relationships, file locations/diffs, and tool progress. Existing JSON metadata remains available alongside stable fields. Question responses may use top-level `answers`; account execution credentials remain input-only.

Event iterators poll the durable `session/transcript` with pages of 256 and a default 200 ms interval. They verify session identity and strictly increasing sequence numbers. For an existing session, pass the sequence captured **before** submitting a prompt to wait only for that new turn. A default ten-minute polling deadline and request timeout prevent indefinite waits. Breaking iteration stops polling; use cancellation where supported. `waitForTurn` collects one turn, while event iterators process pages with backpressure.

## Supported transports

| Client | HTTP | Owned stdio | WebSocket | Unix socket |
| --- | --- | --- | --- | --- |
| TypeScript, Node 22+ | Yes, Rust | Yes | No | No |
| TypeScript, browser | Yes, Fetch | No | No | No |
| Python 3.11+ | Yes, Rust | Yes, desktop | No | No |
| Go 1.22+, cgo | Yes, Rust | Yes, desktop | No | No |
| Swift 6, macOS 13+/iOS 16+ | Yes, Rust | Yes, macOS | No | No |

Transcript polling is the maintained event path in these clients. The runtime's WebSocket transport remains available to independent clients; the native desktop can also use its Unix socket and SSH channels independently of these SDKs. HTTP response bodies and stdio frames default to 16 MiB. Owned stdio calls are serialized inside Rust; each request deadline includes lock acquisition, writing, and response reading. A timeout, cancellation, malformed frame, or dropped in-flight read closes the owned process. A structured RPC error preserves the stream for later requests. Notifications do not consume a response.

Go contexts, Node `AbortSignal`, Swift task cancellation, and Python `threading.Event` all cancel active native I/O and polling waits. `waitForTurn` uses the same Rust completion logic for every native binding and limits the collected JSON to the client's response limit; use the event iterator for larger turns. Request limits accept 1–256 MiB, and deadlines accept 1 ms–one year. Closing a client interrupts active calls and cursors, including idle polling waits. Each binding offers `dispose`/`Dispose`; closing a runtime session remains a separate RPC operation.

No npm, PyPI, Go proxy, or Swift registry release is claimed. Install from these directories or the local artifacts produced below.

## Local distribution

```sh
python3 sdk/tools/package_local.py --binary target/release/swarmie
```

This builds native libraries in release mode, separate contracts and host-specific TypeScript npm tarballs, a Python platform wheel, and Go/Swift archives with their native libraries. On macOS, the Swift archive includes a static XCFramework for macOS x86_64/arm64, iOS arm64, and both simulator architectures. It exports the canonical schema, and bundles the supplied runtime executable with its license. Output defaults to an external temporary directory. The runtime archive is for the current host platform and architecture; the script refuses an incompatible executable. It does not publish packages. Runtime executable bundling remains for the current host; Apple SDK packaging additionally cross-compiles the XCFramework on macOS. Python packaging requires installed `pip`, `setuptools>=68`, and `wheel`; TypeScript packaging requires Node, Bun, and npm.

Use `cargo build -p app --release` first for a production executable. A debug executable can be explicitly supplied for local verification and is labeled as a local build. Separate macOS binaries must be built on macOS; code signing, notarization, installer creation, and registry publication remain release tasks.

## Verification

```sh
python3 scripts/client-conformance.py --swift-host macos-build-host
```

The runner starts a disposable actual daemon with `tests/fixtures/client-script.json`, checks generated contracts against it, and runs each client's HTTP lifecycle tests. Node and Python additionally run actual owned stdio sessions; Swift tests also exercise owned-process timeouts and cancellation. Swift tests run through SSH on the requested macOS host with a loopback remote forward. Unit checks cover response identity, structured errors, limits, deadlines, and input-only credentials. Disposable verification state stays outside the repository. Native libraries are staged in ignored SDK build directories. CI runs Linux conformance and local macOS Swift conformance (`--swift-local`) without mounting private documentation.
