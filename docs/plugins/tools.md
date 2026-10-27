# Tools

`swarmie-tools` stores tool metadata and routes calls to Tembra services. A
`ToolDefinition` pairs a model-facing `ToolSpec` with a `ToolProvider` address.
Tembra owns service resolution, principal checks and resource lifetime.

## Register a tool

```rust
use swarmie_tools::{Catalog, ToolDefinition, ToolName, ToolProvider, ToolSpec};

fn register() -> tembra::Result<()> {
    let catalog = Catalog::default();
    let lease = catalog.register(ToolDefinition::new(
        ToolSpec {
            name: ToolName::new("read_file")?,
            description: "Read a file.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {"path": {"type": "string"}},
                "required": ["path"]
            }),
        },
        ToolProvider::new("files.tools", vec!["main".into()])?,
    )?)?;
    assert_eq!(catalog.snapshot()?.tools.len(), 1);
    drop(lease);
    assert!(catalog.snapshot()?.tools.is_empty());
    Ok(())
}
```

The provider service must exist separately. Registering metadata does not install
a service or grant permission to call it. Duplicate tool names return a conflict.
Keep the `Registration` lease for as long as the tool must remain available.
Dropping it, or calling `withdraw`, removes that registration.

Use `register_owned(&declaration, definition)` to bind registration to a managed
Tembra resource. Retain the resource in the owner's environment. For names shared
across principals, declare it once in `Orb::create`, rather than once per service
object.

## Connect consumers and providers

Install `swarmie_tools::orb(host.clone(), catalog)` to expose the catalog through
`SERVICE`. A consumer can bind `CatalogService` with key `[INSTANCE_KEY]`, then call
`list` to obtain a `Selection`. `Selection::select` chooses named tools;
`Selection::specs` returns their model-facing definitions.

The ordinary `orb` exposes one intentionally shared metadata projection. For
private catalogs, use `orb_with_catalog(host, select)`; its factory calls `select`
with the Tembra service object's principal. Return that principal's own catalog
or refuse acquisition. A catalog selected for one principal must not contain
another principal's private descriptions or schemas. This visibility boundary is
separate from the provider's dispatch and receipt authorization.

`CatalogService` implements `Tools`, so Agent Loop can use it directly. To use one
provider without a catalog, bind `ToolService` or implement `Tools` directly. None
of these paths needs a scheduler, storage or Plugin Manager.

With the optional `plugin-manager` feature, `Catalog::from_plugins(index)` reads
active `ToolDefinition` contributions. Plugin Manager remains independent of tool
types. Manager-controlled plugins should contribute metadata through this index so
that disabling a plugin withdraws new admission. Native orb declarations remain
installed until the host restarts; disabling a plugin does not unload them.

A provider service implements the typed `ToolMethod` operations:

| Operation | Input | Result |
| --- | --- | --- |
| `Validate` | `(ToolSpec, ToolCall)` | `ToolDecision` |
| `Execute` | `(OperationId, ToolCall)` | `ToolResult` |
| `Recover` | `(OperationId, ToolCall)` | `Recovery<ToolResult>` |

Providers validate final arguments and enforce authority for execution and
recovery. The catalog does not validate JSON against every parameter schema.
Catalog membership is not an authorization decision.

## Dispatch and recover

`Tools::prepare` returns a decision and, for routed calls, the confirmed provider
address. Save that address with the operation ID and final call before external
dispatch. Pass all three through `ToolExecution` on execution or recovery.

The catalog verifies the definition at preparation and checks availability again
before dispatch. If the tool is withdrawn before dispatch, it returns a confirmed
blocked result. A retry must use `prepare_retry`: the catalog compares the saved
provider before it sends pending arguments for validation. It does not send them
to a replacement provider.

After dispatch, withdrawal cannot prove an outcome. Recovery goes to the saved
provider even when the tool is no longer in the current selection. Managed catalog
references retain provider objects across preparation, execution and recovery;
they release when the catalog object's resource closes. Routed acquisition uses
the calling root's principal. Recovery must preserve that principal as well as the
saved operation and provider. Providers still enforce authority when old operation
receipts are read.

These in-memory references do not survive a process restart. After restart,
recovery must resolve the saved provider again under the original principal. The
provider must preserve or reconcile its own operation outcomes.

`ToolResult::Success`, `Failed` and `Blocked` are confirmed outcomes. A returned
error has no confirmed outcome. `Recovery::Unknown` does not permit replay; only
`SafeToRetry` permits another attempt. The default recovery result is `Unknown`.
The tools library stores no durable receipts of its own.

## Explain withdrawn tools to the model

At the next user-initiated generation, compare the new selection with the previous
one and prepare request-local context:

```rust
use swarmie_tools::{AgentContext, Selection};

fn prepare(
    current: &Selection,
    previous: &Selection,
    context: AgentContext,
) -> AgentContext {
    current.prepare_context(previous, context)
}
```

This adds an instruction naming tools that are no longer selected. It leaves
canonical messages intact. The application calls it explicitly; internal Agent
Loop turns do not add notices automatically. The comparison detects removed names,
not every change to provider addresses or schemas.
