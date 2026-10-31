# Tools

`swarmie-tools` stores tool metadata and routes calls to Tembra services. A
`ToolDefinition` pairs a model-facing `ToolSpec` with a `ToolProvider` address.
Tembra owns service resolution, principal checks and resource lifetime.

## Coding tools

Enable the native `coding` feature to use these tools:

| Tool | Operation |
| --- | --- |
| `read` | Read text lines with offset and output limits. |
| `write` | Replace a file through an atomic rename. |
| `edit` | Apply an ordered set of replacements before writing the file. |
| `search` | Find text with a regular expression and ignore rules. |
| `list` | List files with depth, glob and ignore filters. |
| `shell` | Run a command with timeout, output limits and process-tree cleanup. |
| `delete` | Delete a file. Refuse directories. |
| `move` | Move a file. Replace an existing destination only when requested. |

The host selects a `Workspace`, process `Launcher` and admission policy.
`CodingTools::new` requires a policy for the exact final definition and arguments.
The policy runs during preparation, execution and recovery receipt access.

```rust
use futures::FutureExt;
use std::path::PathBuf;
use swarmie_tools::{
    ToolDecision, ToolPolicy,
    coding::{CodingConfig, CodingTools},
};

fn coding(base: PathBuf, principal: String) -> tembra::Result<CodingTools> {
    let config = CodingConfig::local(base)
        .map_err(|error| tembra::Error::invalid("coding.base", error.to_string()))?;
    let policy = ToolPolicy::new(move |context, _spec, _call| {
        let principal = principal.clone();
        async move {
            context.check()?;
            Ok(if context.principal()? == principal {
                ToolDecision::Allow
            } else {
                ToolDecision::Block("coding tools are not permitted".into())
            })
        }
        .boxed_local()
    });
    CodingTools::new(config, policy)
}
```

Install the returned `orbs` with the host. Register `tools` through its scoped
catalog or Plugin Manager lifecycle. A dependency on this library enables no
tools by itself. Use the policy to apply command and path permissions.

The standard configuration confines file paths to the base, after symlink
resolution. This check is not a race-proof OS sandbox. Shell uses the base as
its working directory. Commands can access other paths unless the host supplies
a sandbox through `Launcher` or another `Workspace`.

Reads, writes and edits share bounded content tracking. Edits refuse ambiguous
matches. Shell supports optional live output and spill storage. Tembra
cancellation stops its process tree. The provider waits for its worker to
finish. Cancellation cannot undo file changes or commands that already ran.
The eight coding tools support macOS, Linux and Windows. On macOS and Linux,
`shell` uses `sh -c` with a new session and process group. On Windows, it uses
Windows PowerShell with `-NoLogo -NoProfile -NonInteractive -Command` and UTF-8
output. Write commands in the selected interpreter's syntax; installing Git Bash
or WSL is not required. The tool description names the interpreter.

On Windows the launcher creates the child suspended, assigns it to an owned
Job Object, then resumes it. Timeout, cancellation and cleanup terminate the
job and wait for process settlement, including descendants after the leader
exits. Windows termination is immediate; Unix first sends `SIGTERM`, then
`SIGKILL` after the grace period. Setup failure refuses execution without a
child-only fallback. A cleanup error is an unconfirmed outcome, not authority
to replay the command. Job ownership is not a security sandbox; commands can
still cause effects through external services.

File paths use the platform's native rules. Windows accepts drive-qualified,
UNC and extended paths, subject to host permissions and the configured base.
Results can contain native separators and canonical Windows path prefixes.
UTF-8 file content, Unicode names, CRLF preservation, replacement writes and
read-only refusal follow the same tool contracts. Replacement writes copy the
existing Windows DACL and its inheritance protection before publishing data.
They refuse replacement if access permissions cannot be copied. This does not
preserve ownership, audit rules, alternate streams or other backup metadata. Symbolic links and directory
junctions are resolved for base checks. Unix command suggestions are omitted
on Windows. Other native platforms require their own process backend.

`read` returns text. The integration has no image output, message attachments,
clipboard access, remote transport or automatic filesystem checkpoints.

## Author a tool

Implement `Tool` with owned arguments, an `IntoToolOutput` result and an
asynchronous `call`. Use `args_schema` with a `schemars::JsonSchema` arguments
type. Mount the author with `ToolHandler::new` and `tool_orb`.

The adapter checks the tool name, definition, schema and typed arguments before
effects. The schema checker supports the subset described in `validate`.
Domain checks and host permissions remain separate. Annotations and footprints
describe the operation. They grant no permission, parallel execution or retry.

`ToolContext` has typed input data and explicit result metadata. During a call,
`execution`, `operation` and `call` return its actual Tembra context and confirmed
intent. Serialization excludes this runtime authority. Input changes stay in
that call. `execute_with` returns result metadata to a direct host caller.
Terminal hooks receive the metadata for routed calls. The host decides whether
to store it.

Strings remain text. Explicit JSON remains structured data.
`ToolExecutionError::failed` carries model-visible feedback.
`ToolExecutionError::refused` becomes a blocked result. Arbitrary source errors
keep operator diagnostics separate from model feedback. Their effects remain
unconfirmed. The default `recover` is `Unknown`. Override it to reconcile an
operation receipt. Only an explicit `SafeToRetry` permits another attempt.

## Extend calls with hooks

`tool_orb` declares and binds the descriptors in `hooks::Hooks`:

| Hook | Contract |
| --- | --- |
| `before_call` | Restrict preparation, execution or recovery admission. A block stops the contribution chain. The phase, operation, definition and call must stay unchanged. |
| `after_call` | Observe the original physical source, cancellation state, exact call, outcome and explicit result metadata. |

Use `Orb::contribute` with the particular provider Orb as its target. Declare
the hook requirement on the extension Orb. A coding provider has the name
`swarmie.tools.coding.<name>`. Custom providers declare the same descriptors and
use `ToolHandler::bind_hooks`.

```rust
use swarmie_tools::{ToolDecision, hooks::{Hooks, Phase}};
use tembra::{Orb, core::Requirement};

fn write_guard() -> Orb {
    let hooks = Hooks::default();
    let target = "swarmie.tools.coding.write";
    Orb::new("write-guard", 1)
        .requires(Requirement {
            orb: target.into(),
            api_version: 1,
            methods: Default::default(),
            hooks: ["before_call".into()].into(),
        })
        .contribute(target, &hooks.before_call, 0, |_, mut input| async move {
            if input.phase == Phase::Execution
                && input.call.arguments.get("path").and_then(|path| path.as_str())
                    == Some("protected.txt")
            {
                input.decision = ToolDecision::Block("protected file".into());
            }
            Ok(input)
        })
}
```

Admission hooks must be pure. They can run more than once for one operation.
Argument changes belong in Agent Loop preparation, before intent confirmation.
Provider hooks cannot rewrite a pending call or override a previous block.

Tembra owns contribution plans, versioning and physical observation. Terminal
observation preserves original cancellation and does not restore closed storage
frames. An observer cannot replace an outcome or authorize replay.
Use `ToolHandler::observation_failure` or the provider's `hook_failure` method
to read the last observation error. Hook effects have no exactly-once guarantee.

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
