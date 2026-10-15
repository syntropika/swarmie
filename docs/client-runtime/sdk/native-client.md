# Native SDK boundary

The language SDKs marshal the daemon's canonical protocol through `swarmie:client/native@0.1.0` in `sdk/wit/client.wit`. WIT is the interface description, while the selected lowering is a native Rust library. It is not the WebAssembly Canonical ABI. Domain parameters, results, and events remain JSON strings described by `packages/contracts/protocol.json`, avoiding a parallel recursive domain schema.

```text
Node ── Node-API ─────────┐
Python ── ctypes ── C ABI ┤
Go ── cgo ──────── C ABI ├── Rust client ── HTTP / owned stdio ── runtime
Swift ── C module ─ C ABI┘
Browser ── Fetch ─────────────────────────────────────────────── runtime
```

The Rust build generates records, enums, operation arguments, trait signatures, and exhaustive dispatch from WIT with `wit-parser`. This checks both syntax and implementation signatures. Language wrappers use the generated dispatch contract and are qualified against real runtime responses. The browser entry remains an independent portable transport and shares generated protocol types and convenience methods; browser HTTP qualification covers identity, body limits, and deadlines.

## Native lowering and ownership

`swarmie_client_abi_version()` returns 1. `swarmie_client_execute()` accepts a borrowed, NUL-terminated UTF-8 JSON command and returns an owned UTF-8 JSON reply. The caller frees that reply exactly once with `swarmie_client_free()`. Null input produces a typed error; null free is accepted. No borrowed Rust data or raw resource pointers cross the boundary. Panics are contained, and transport diagnostics exclude bearer credentials and request contents.

The lowering uses snake_case for WIT names and an `action` discriminator. Replies contain either `ok` or `error`; the error includes kind, message, optional RPC code, and optional JSON data. An RPC result of null is carried as the JSON string `"null"`; a control operation's successful unit result is represented by `ok: null`.

Clients, operations, and event cursors have process-local integer handles. Handles are never recycled and remain within JavaScript's exact integer range. Invalid or released handles fail safely; release is idempotent. Create an operation with `prepare` before scheduling work, cancel it through `cancel`, and release it after completion or failure. Preparation ensures cancellation before dispatch cannot race with operation registration. Bindings remove cancellation hooks and release operations in their cleanup paths.

Releasing a client closes its transports and interrupts active requests and event waits. Releasing an event cursor cancels its pending pull. A cursor retains its client until released but cannot revive a closed client. Language finalizers provide cleanup for abandoned references; explicit disposal is the deterministic path. No call is automatically retried, including after uncertain prompt delivery.

## Transport and event invariants

HTTP accepts credential-free HTTP/HTTPS endpoint URLs and normalizes the root path to `/rpc`. Redirects are rejected. The deadline covers headers and streamed body reads; the response size is checked before accumulating each chunk. Bearer tokens stay in memory. Requests receive unique client-prefixed IDs, and responses must match JSON-RPC version and identity, contain a result or structured error, and never contain both. Null results are valid.

Owned stdio holds one request lock covering writes and framed reads. The request deadline includes time waiting for that lock. Frames are bounded as bytes, including partial lines; valid notifications are ignored. Structured RPC errors leave the transport open. Cancellation, timeout, transport failure, or dropping an active read closes and reaps the owned child to prevent late-response ambiguity. Closing one owned process never shuts down an independently connected HTTP daemon.

Cursors poll durable transcripts in pages of 256, hold at most one page, and reject wrong session identities, missing/non-increasing sequences, and oversized pages. A total deadline includes polling sleeps and transport reads. Cancellation and client closure interrupt those sleeps. Turn collection returns at `turn.ended`, fails at an earlier `session.ended`, and bounds collected JSON by the configured response limit. Stream events directly for larger turns.

## Build and packaging

Start from the parent root with `just sdk-build`. The builder stages a Node-API module, Python shared library, Go static library, and Swift current-host shared library. Host artifacts are ignored source build outputs. TypeScript tarballs and Python wheels contain matching host libraries; Go archives include a static library and C header. These artifacts require no hosted repository, private documentation, or credentials to install.

`just sdk-build-apple` additionally uses the pinned Rust toolchain and Xcode to build a static XCFramework for macOS x86_64/arm64, iOS arm64, and simulator x86_64/arm64. SwiftPM selects the binary target when present. It uses no unsafe linker flags, so another Swift package can consume it. Application signing and real-device iOS integration remain separate qualification work. Arbitrary owned child processes are a desktop capability.

Use `just sdk-check` for formatting, Clippy, Rustdoc, and TypeScript compilation. `just sdk-test macos-build-host` qualifies every binding against a disposable real daemon, including remote Swift through a loopback SSH forward. CI also supports `--swift-local` on macOS. Rust behavior tests cover envelope validation, bounds, redirects, cancellation races, event ordering, deadlines, and turn completion. Package verification additionally installs the npm archives into a separate consumer.
