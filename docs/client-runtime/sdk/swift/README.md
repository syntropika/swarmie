# Swift runtime client

Run `just sdk-build-apple` on a macOS build host, or extract the Swift archive containing `CSwarmie.xcframework`. Then add a local SwiftPM dependency:

```swift
.package(path: "/absolute/path/sdk/swift")
```

```swift
import Foundation
import SwarmieClient

let client = try Client(url: URL(string: "http://127.0.0.1:7457")!)
defer { try? client.dispose() }
let session = try await client.spawn(InputSpawnOptions(cwd: "/absolute/project", harness: "codex"))
do {
    let requestKey = UUID().uuidString // Persist this key before sending.
    _ = try await client.prompt(sessionID: session.id, text: "Describe the project.", idempotencyKey: requestKey)
    for try await event in try client.events(sessionID: session.id, options: EventOptions(afterSequence: session.sequence)) {
        if event.type == "text.delta" { print(event.data?["text"]?.stringValue ?? "", terminator: "") }
        if event.type == "turn.ended" { break }
    }
    try await client.close(sessionID: session.id)
} catch {
    try? await client.close(sessionID: session.id)
    throw error
}
```

Set `ClientOptions(token:)` for bearer authentication. The actor supports concurrent requests. `call(_:params:as:)` exposes every RPC method using `Encodable` input and `Decodable` output. Generated domain objects are `Codable` and `Sendable`; variant/provider payloads use `JSONValue`. `RPCError` retains the server code, message, and data.

Requests default to 30 seconds and bounded 16 MiB response accumulation. `EventStream` fetches pages on demand, checks ordering, and stops on task cancellation. The default deadline is ten minutes with 200 ms polling and 256-event pages. Use the saved sequence from before submission to exclude previous turns.

Requires Swift 6 and macOS 13+/iOS 16+. macOS compilation and HTTP conformance are covered by the conformance runner on a configured macOS host; iOS application integration is not verified. The static XCFramework contains macOS x86_64/arm64, iOS arm64, and simulator x86_64/arm64 slices. HTTP and owned stdio call the shared Rust engine; use `Client(command:arguments:options:)` for an owned desktop process and `dispose()` for native resources. iOS application sandboxes do not support arbitrary child processes. WebSocket and Unix socket are not implemented. A current-host dynamic library is available for local source verification when the XCFramework has not been staged. No Swift registry publication is claimed.

Each intentional prompt requires its own persisted idempotency key. Reuse the original key and content to obtain the retained receipt without dispatching again. Query `session/submission` after a lost response; this read does not open a native session. `delivery` distinguishes `not_sent`, `in_flight`, `accepted`, and `unknown`. Acceptance does not mean completion. An uncertain receipt blocks new work; clients do not automatically resend. Keys are scoped to a personal node session and survive event-history trimming while its submission ledger is retained.

Use `session/submission/reconcile` (or the client reconciliation helper) to check an already open native connection without resending work. Explicitly resume a closed session first. Native absence is not rejection; partial evidence and unsupported adapters keep uncertain work paused. Settled receipts require no native lookup. Check `capabilities.submissionReconciliation` for qualified lookup support.
