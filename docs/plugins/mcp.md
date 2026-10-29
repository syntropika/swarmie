# MCP

`swarmie-mcp` connects to local MCP servers through stdio and remote servers through
Streamable HTTP. The library discovers tools and supplies a Tools provider. The
embedding host supplies a Tokio executor, execution permissions and the principal.

## Configuration

Each `Configuration` contains a `ServerId`, an `Endpoint`, a `Protocol` and `Limits`.
The `ServerId` contains 1 to 64 ASCII letters, digits, underscores or hyphens.
Configuration creation and deserialization do not start a connection.

`Endpoint::Local` specifies a program, arguments and an optional working directory.
The server program and its dependencies must already be installed. The program runs
without a shell and inherits the host environment. Local execution has the host's
permissions. The client discards stderr.

`Endpoint::Http` specifies an HTTP(S) URL and a map of custom headers. URL credentials
and fragments are rejected. The map permits up to 32 headers, with up to 4096 bytes
per value. Invalid names, line breaks and duplicate names are rejected. Header names
are compared without case. Authentication, cookie, host and MCP transport headers
cannot be set through this map. Do not put secrets in URLs, headers or command arguments.

The client uses `rmcp` 3.8.0 with two explicit protocol profiles:

| Profile | Protocol version | Connection lifecycle |
| --- | --- | --- |
| `Protocol::Current` | `2026-07-28` | Discover |
| `Protocol::Previous` | `2025-11-25` | Initialize |

The selected version must match the server's negotiated version. A mismatch stops
connection setup before tool discovery. The client does not downgrade the protocol
automatically. HTTP redirects, automatic HTTP retries and session-expiration replay
are disabled. The two-endpoint HTTP+SSE transport is not supported.

`Limits::connect_timeout` bounds negotiation and all discovery pages together.
Each page also has a request timeout. Catalog limits bound page and tool counts.
Cancellation during connection setup stops negotiation or discovery and waits for cleanup.

## Composition

Each `ServerBinding` supplies a configuration, a principal and a Tools catalog.
Use `swarmie_tools::orb_with_catalog` to select the catalog for each principal.

```rust,no_run
use swarmie_mcp::{Configuration, Endpoint, Limits, Protocol, ServerBinding, ServerId};
use swarmie_tools::Catalog;

fn compose(host: &tembra::Host, service_url: String, principal: String)
    -> tembra::Result<()> {
    let catalog = Catalog::default();
    let selected = catalog.clone();
    let owner = principal.clone();
    host.install(vec![
        swarmie_tools::orb_with_catalog(host.clone(), move |caller| {
            if caller == owner {
                Ok(selected.clone())
            } else {
                Err(tembra::Error::invalid("app.catalog_owner", "unbound principal"))
            }
        }),
        swarmie_mcp::orb(vec![ServerBinding {
            configuration: Configuration {
                id: ServerId::new("documents")?,
                endpoint: Endpoint::Http { url: service_url, headers: Default::default() },
                protocol: Protocol::Current,
                limits: Limits::default(),
            },
            principal,
            catalog,
        }])?,
    ])
}
```

The MCP service name is `swarmie_mcp::SERVICE`. Its reference key is `[server_id]`.
Duplicate server IDs within one principal are rejected. Separate principals can use
the same server ID with separate catalogs. The ordinary Tools `orb` exposes one
shared catalog. Catalog visibility does not grant execution permission.

Tools owns the catalog and provider routing. Tembra owns service references,
resources and physical cleanup. PluginManager is outside the tool execution path.
The MCP library also works without PluginManager, Agent Loop, a scheduler or storage.

## Connection methods

Reference acquisition and `ready()` create a disconnected resource. They do not
start a process or open a network connection.

| Method | Input | Behavior |
| --- | --- | --- |
| `McpMethod::Connect` | `()` | Connects, discovers tools and publishes the complete catalog. Returns `Observation`. |
| `McpMethod::Describe` | `()` | Returns `Observation` without opening a connection. |
| `McpMethod::Disable` | `()` | Removes catalog entries and stops new calls. |
| `McpMethod::Result` | `(OperationId, ToolCall)` | Returns the saved original response, if available. |

`Connect` publishes registrations only after complete discovery. A registration
failure removes partial registrations. A disable during setup prevents later publication.

`Observation` contains a typed connection status, an optional protocol version,
server metadata and tool metadata. The protocol and server fields are absent before
connection. Metadata is a connection-time snapshot. It does not prove continuous
connection health. Connections do not refresh catalogs or reconnect automatically.

`Disable` lets admitted calls finish. Reference acquisition does not enable a
disabled connection. Explicit `Connect` can enable a healthy disabled connection
and publish its tools again. The connection keeps its existing operation receipts.
An unavailable connection requires physical teardown before a new connection can start.

## Settings

`settings::settings_orb` composes the `swarmie.mcp.settings` service at key `main`.
The embedding supplies a `DefinitionStore` and the same principal-specific catalog
that Tools uses. `MemoryDefinitions` keeps definitions in memory. A storage adapter
must compare the expected inventory before it replaces that inventory.

Each definition has a stable ID, a display name, an endpoint, a protocol profile,
desired enablement and a revision. Definitions belong to one authenticated user
on one execution node. Project and workspace configuration are not supported.

| Method | Input | Behavior |
| --- | --- | --- |
| `list` | `()` | Returns definitions and observations without connecting. |
| `save` | `(SaveDefinition,)` | Creates or updates a definition after a revision check. The saved definition is disabled. |
| `enable` | `(ServerAction,)` | Saves desired enablement, then connects and discovers tools. |
| `disable` | `(ServerAction,)` | Stops new calls, then saves disabled state. |
| `remove` | `(ServerAction,)` | Stops new calls and removes the definition. It does not uninstall a program. |

`SaveDefinition::expected_revision` is absent for a new definition. An update must
provide the current revision. `ServerAction` contains the ID and revision. Stale
actions fail. Save cannot change a local endpoint into an HTTP endpoint or the reverse;
remove the definition first. A failed connection can leave desired enablement set
to true. Observed connection state remains separate.

Configured providers use service `swarmie.mcp.configured` and key `[id, revision]`.
Before connection and catalog publication, the provider checks that its definition
is current and enabled. Editing stops admission to the old revision. Tools can keep
its original reference to reconcile an earlier operation. Old receipts are not moved
to the replacement provider. Tembra owns both resources and their physical cleanup.

The client shows these definitions in **Settings → Plugins → MCPs**. Add accepts a
remote URL with non-secret headers, or an installed local command with exact arguments
and an optional working directory. Arguments are entered as a JSON string array;
no shell parsing occurs. Search filters the current inventory. The switch shows
desired enablement; the row separately shows the last observed state and tool count.
Refresh reads state. Connect is explicit after a restart or connection loss.

The client binds its settings reference to the selected node, principal and session
generation. A scope change clears the old projection. A late response cannot populate
the replacement scope. After a write, the client reads the inventory to reconcile the
result. It never repeats a write because its response is unknown.

## Tool execution

Tool names use `mcp.<server_id>.<remote_tool_name>`. Each `ToolProvider` contains the
MCP service name and its stable server key. Tembra resolves the provider under the
calling principal. The principal is separate from the model's agent identity.

Tools prepares the final hook-adjusted arguments against the selected provider
before dispatch. The MCP provider checks the discovered definition and requires
an argument object. The server validates the parameter schema. Use `CatalogService`
with Agent Loop as described in [Tools](tools.md).

Complete results retain content and structured content. The model-facing
`ToolResult` excludes private protocol `_meta`. This removal covers content blocks
and embedded resource contents. Business fields named `_meta` in structured data
remain unchanged.

The original response is saved before model projection. A projection failure keeps
that response and does not permit replay. `Result` requires the original principal,
operation ID and exact call intent. Protocol errors expose their code and message,
without arbitrary private error data.

## Recovery

Each in-memory receipt binds an operation ID to its principal, call identity and
arguments. Reuse with different intent fails. A repeated operation returns its
saved result without another MCP call. An uncertain operation is never replayed
automatically. Recovery returns a saved result or `Unknown`.

A known tool or protocol error is a confirmed failed outcome. Transport failures,
timeouts and unsupported continuations can leave an uncertain outcome. Cancellation
does not reverse external effects. A dispatched call retains its physical reply waiter
until a response, error or configured deadline. A correlated final response remains
available for recovery even when its caller was cancelled. Caller cancellation preserves
the original cancellation error; it does not replace a retained tool receipt.
Physical settlement can keep Stop, steer or host shutdown pending until the configured
request timeout, which defaults to 30 seconds. A deadline never authorizes replay.

Transport errors and ordinary request timeouts mark the connection unavailable. A deadline
after logical cancellation retains an unknown receipt while leaving a still-open transport
available for other calls. An uncertain operation is never dispatched again. An unavailable
connection blocks new calls before dispatch. The mounted provider removes catalog registrations after
Execute settles. If a transport closes while idle, entries remain until the next
Execute attempt. That attempt is blocked before dispatch and removes the entries.
Recovery still reaches the original provider after catalog removal.

`Describe`, `Result` and `Recover` never start a connection, including after physical
disposal. Unknown outcomes do not permit automatic retries. Typed `Reason` values
identify invalid input, unavailable connections, unknown outcomes, cancellation,
projection failures and concurrent connection attempts.

Receipts have a configured count limit. New operations are refused at that limit;
uncertain receipts are kept. Receipts do not survive physical disposal or restart.

## Cleanup

Tembra waits for admitted calls before resource closure. Closure waits for protocol
cleanup and terminates and reaps the owned local process. Failed local startup also
waits for process cleanup. Cleanup covers the directly owned child process.
It does not terminate a process tree or reverse remote effects.

Independent users of `Connection::connect` supply a valid call context. To cancel
startup, cancel that context and continue polling until cleanup finishes. Dropping
the connection future requests process termination but does not confirm reaping.
Await `close()` and keep the executor running until it returns. Managed invocations
keep physical ownership when a caller stops waiting.

## Interface limits

The library does not expose resource discovery, prompt APIs, MCP Apps rendering,
interactive continuations, Tasks or authentication flows. The embedding host supplies
configuration persistence. This crate does not install server dependencies. The settings
UI does not provide credential fields or OAuth.
