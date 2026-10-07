# Swift runtime client

Add a local SwiftPM dependency:

```swift
.package(path: "/absolute/path/packages/clients/swift")
```

```swift
import Foundation
import MPAIClient

let client = try Client(url: URL(string: "http://127.0.0.1:7457")!)
let session = try await client.spawn(InputSpawnOptions(cwd: "/absolute/project", harness: "codex"))
do {
    _ = try await client.prompt(sessionID: session.id, text: "Describe the project.")
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

Requires Swift 6 and macOS 13+/iOS 16+. macOS compilation and HTTP conformance are covered by the conformance runner on a configured macOS host; iOS application integration is not verified. Only HTTP is provided; stdio, WebSocket, and Unix socket are not implemented. No Swift registry publication is claimed.
