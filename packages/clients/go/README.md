# Go runtime client

Use the local module until a release has a public module path:

```go
require mpai.local/client v0.0.0
replace mpai.local/client => /absolute/path/packages/clients/go
```

```go
client, err := mpai.New("http://127.0.0.1:7457", mpai.Options{})
if err != nil { panic(err) }
ctx := context.Background()
session, err := client.Spawn(ctx, mpai.InputSpawnOptions{Harness: "codex", Cwd: "/absolute/project"})
if err != nil { panic(err) }
defer client.Close(ctx, session.Id)
if _, err = client.Prompt(ctx, session.Id, "Describe the project."); err != nil { panic(err) }
err = client.Events(ctx, session.Id, mpai.EventOptions{AfterSequence: session.Sequence}, func(event mpai.RuntimeEvent) bool {
    fmt.Println(event.Type, string(event.Data))
    return event.Type != "turn.ended"
})
```

Import the module as `mpai "mpai.local/client"`. `Call(ctx, method, params, &result)` covers every RPC method. `errors.As` recovers `*mpai.RPCError` with its code and opaque JSON data. Session objects are generated from the runtime schema; variant/provider payloads use `json.RawMessage`. `TextContent` and `ImageContent` construct structured prompt blocks.

Set `Options.Token` for bearer authentication. Request timeouts default to 30 seconds and responses to 16 MiB. Context cancellation interrupts requests and event polling. Polling defaults to ten minutes, 200 ms intervals, and pages of 256 events. Return false from the callback to stop. Only HTTP is provided; stdio, WebSocket, and Unix socket are not implemented. This source module is local and has no third-party dependencies.
