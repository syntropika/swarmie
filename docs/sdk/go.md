# Go runtime client

Run `just sdk-build` from the source root, or extract a matching native archive. Enable cgo and provide a C compiler. Use the local module until a release has a public module path:

```go
require swarmie.local/client v0.0.0
replace swarmie.local/client => /absolute/path/sdk/go
```

```go
client, err := swarmie.New("http://127.0.0.1:7457", swarmie.Options{})
if err != nil { panic(err) }
defer client.Dispose()
ctx := context.Background()
session, err := client.Spawn(ctx, swarmie.InputSpawnOptions{Harness: "codex", Cwd: "/absolute/project"})
if err != nil { panic(err) }
defer client.Close(ctx, session.Id)
requestKey := "unique-persisted-request-key"
if _, err = client.Prompt(ctx, session.Id, "Describe the project.", requestKey); err != nil { panic(err) }
err = client.Events(ctx, session.Id, swarmie.EventOptions{AfterSequence: session.Sequence}, func(event swarmie.RuntimeEvent) bool {
    fmt.Println(event.Type, string(event.Data))
    return event.Type != "turn.ended"
})
```

Import the module as `swarmie "swarmie.local/client"`. `Call(ctx, method, params, &result)` covers every RPC method. `errors.As` recovers `*swarmie.RPCError` with its code and opaque JSON data. Session objects are generated from the runtime schema; variant/provider payloads use `json.RawMessage`. `TextContent` and `ImageContent` construct structured prompt blocks.

Set `Options.Token` for bearer authentication. Request timeouts default to 30 seconds and responses to 16 MiB. Context cancellation interrupts requests and event polling. Polling defaults to ten minutes, 200 ms intervals, and pages of 256 events. Return false from the callback to stop. Use `NewStdio(command, args, options)` for an owned process. `Dispose` closes native resources separately from the session-closing RPC. HTTP and stdio execute in the shared Rust engine; WebSocket and Unix socket are not implemented. Source builds link the staged static Rust library on Linux or macOS. The Go module remains local; Go packages have no third-party dependencies.

Each intentional prompt requires its own persisted idempotency key. Reuse the original key and content to obtain the retained receipt without dispatching again. Query `session/submission` after a lost response; this read does not open a native session. `delivery` distinguishes `not_sent`, `in_flight`, `accepted`, and `unknown`. Acceptance does not mean completion. An uncertain receipt blocks new work; clients do not automatically resend. Keys are scoped to a personal node session and survive event-history trimming while its submission ledger is retained.

Use `session/submission/reconcile` (or the client reconciliation helper) to check an already open native connection without resending work. Explicitly resume a closed session first. Native absence is not rejection; partial evidence and unsupported adapters keep uncertain work paused. Settled receipts require no native lookup. Check `capabilities.submissionReconciliation` for qualified lookup support.
