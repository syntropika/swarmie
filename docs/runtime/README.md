# swarmie

A headless Rust runtime for installed agent tools. Use the Rust API, an interactive CLI, or a persistent JSON-RPC daemon. Harness integrations use native structured protocols; client code consumes one ordered stream of runtime events.

## Start

Requires Rust 1.88 or later. Install the harness you want to use and authenticate it with its own CLI, or configure a compatible inference endpoint.

```sh
cargo build --workspace
./target/debug/swarmie discover
./target/debug/swarmie probe codex
./target/debug/swarmie quotas codex .
./target/debug/swarmie run codex . --prompt "Explain this project"
./target/debug/swarmie run pi .
./target/debug/swarmie run claude . --prompt "Review this project"
./target/debug/swarmie run opencode . --model provider/model
```

`run` without `--prompt` is interactive. Supported commands are `/steer TEXT`, `/cancel`, `/quit`, and `/answer OPTION [JSON]` for a pending request. Ordinary text starts a turn. A prompt during an active turn returns `Busy`; steering must be explicit and supported by the harness.

One-shot runs default to denying requests that would require interactive input. `--permissions approve` approves offered approval options; structured questions still require an answer through an interactive client or the daemon. `--json` prints normalized events as JSONL. `--timeout` bounds a one-shot turn and defaults to 600 seconds.

Common execution permissions use server-provided `executionProfile` choices. Native settings remain available where no equivalent common mode is supported. Selected, staged and observed settings are separate; unresolved application blocks dependent work. See [execution policy](execution-policy.md).

## Harnesses

The agreed named-agent scope, setup behavior, current transports, and remaining native-integration work are documented in [agent tool support and setup](coding-agent-support.md).

| Harness | Structured interface | Resume | Fork | Steering | Permission requests |
| --- | --- | --- | --- | --- | --- |
| Codex | App Server JSON-RPC | Yes | Yes | Yes | Yes |
| Pi | RPC, version 1.0+ | Yes | Yes | Yes | Native extension dialogs |
| Claude Code | Official Agent SDK through Node worker | Yes | Yes | Yes | Yes |
| Cursor | Official SDK Bridge, Connect HTTP | Yes | Unsupported | Unsupported | Unsupported |
| OpenCode | HTTP API and SSE | Yes | Yes | Unsupported | Yes |
| Antigravity | Native stream-json | Yes | Unsupported | Unsupported | Native auto-execution |
| Grok Build | ACP with reviewed xAI extensions | Negotiated | Unsupported | Reviewed 1.0.52 | Yes |
| Generic ACP | ACP v1 JSON-RPC | Negotiated | Unsupported | Unsupported | Yes |

Gemini, Qwen, Grok, Hermes, and Kiro are registered through ACP. Capabilities are negotiated with the installed version; a catalog entry does not establish authentication or successful model execution. Grok extensions additionally require its native marker and reviewed version. Claude exposes observed background tasks and individual stop controls. Cursor supports local SDK history discovery, read, and import; its CLI store remains separate. Claude and Cursor require the runtime support assets described in [agent tool setup](coding-agent-support.md#runtime-support-assets). A generic ACP command needs no catalog entry:

```sh
./target/debug/swarmie run custom . --executable /path/to/agent --agent-arg=--acp
./target/debug/swarmie serve --acp custom=/path/to/agent --acp-arg=--acp
```

Discovery is passive and reports executable availability. It does not claim authentication or model readiness. Session capabilities reflect adapter support and, for ACP, the handshake. Unsupported operations return an explicit error.

Shared project and user skills use canonical `.agents/skills` folders with native
discovery and invocation adapters for Codex, Claude Code and Pi. See
[shared skills](skills.md) for availability, selection, permissions and lifecycle.

Native executable overrides and launch arguments are available through `--config`:

```sh
./target/debug/swarmie run pi . --config '{"executable":"/path/to/pi"}'
```

`config.args` replaces the native base launch arguments for direct CLI adapters; Pi appends runtime session/resume/model flags. Claude delegates launch flags to the official SDK and uses `config.args` only as a CLI-wrapper prefix. Codex supports `approvalPolicy`, `sandbox`, `modelProvider`, `baseInstructions`, `developerInstructions`, and native `config` at spawn. OpenCode accepts an existing server through `config.url`, with optional `username` and `password`; otherwise swarmie starts a supervised loopback server with a private credential. Use environment-based provider credentials rather than placing secrets in spawn configuration, which is saved in session snapshots.

Pi 1.0+ ends turns at `agent_settled`, including retries and queued work. For a deliberately configured older Pi installation, `config.legacyAgentEnd=true` selects its older completion event; that mode does not provide the newer settled semantics. Blocking Pi extension dialogs are normalized into permission or question requests, with native response translation.

Native JSON process wires and OpenCode HTTP/SSE accept `config.diagnostics` with optional `stallTimeoutMs` and `recording: {"path":"/absolute/trace.jsonl","maxBytes":8388608}`. Recordings redact known credentials and use a bounded, fresh file; opening or filling the trace emits a diagnostic and leaves the connection usable. Stall warnings apply only while an operation or turn is active and never cancel it. OpenCode records query-free request paths, response bodies, and parsed SSE events, without authentication headers or URL credentials.

## Rust API

```rust,no_run
use futures_util::StreamExt;
use swarmie::{EventKind, SpawnOptions};

# async fn example() -> swarmie::Result<()> {
let runtime = adapters::builtin_builder().build();
let agent = runtime.spawn(SpawnOptions::new("codex", ".")).await?;
let mut events = agent.events(); // subscribe before prompting
agent.prompt("Explain this project").await?;

while let Some(event) = events.next().await {
    let event = event?;
    match event.kind {
        EventKind::TextDelta { text, .. } => print!("{text}"),
        EventKind::TurnEnded { .. } => break,
        _ => {}
    }
}
agent.close().await?;
# Ok(()) }
```

`prompt()` returns after acceptance, not completion. `cancel()` interrupts the current turn, `close()` ends the logical session and shuts down its owned connection, and `kill()` forcibly terminates it. Native conversation history remains available for resume. Kill and close can preempt slow acceptance/configuration/compaction. If cancellation cannot obtain an acceptance acknowledgement within five seconds, the engine terminates that session and reports the timeout.

Store session references durably with `storage::SqliteSessionStore`, or provide your own `SessionStore`. The default Rust builder uses `MemoryStore`. The CLI uses `~/.swarmie` on the runtime node; `--state-dir` selects another application data root. This root is separate from user working directories and native Harness history or authentication.

```sh
./target/debug/swarmie sessions
./target/debug/swarmie run codex . --resume RUNTIME_SESSION_UUID --prompt "Continue"
./target/debug/swarmie run codex . --fork RUNTIME_SESSION_UUID --prompt "Explore another approach"
```

Snapshots contain the runtime ID, native provider reference, retained working context, redacted options, capabilities, sequence, metadata, queue, and lifecycle state. The provider owns its native conversation. `SqliteSessionStore` retains snapshots, delivery receipts and ordered canonical events in `state/host.sqlite`, using WAL and full synchronization. `settings.json` pins the store identity and location; `runtime/` contains process locks. Each store has stable store, Environment and owner-scope identities. A configured missing or unsupported database produces an explicit error. Legacy development files remain untouched and require selecting a fresh root; no historical importer is provided. Queue and live metadata changes commit with their events before publication. The atomic `SessionStore::commit(SessionWrite)` interface also supports grouped snapshot, submission and event writes; native calls never run inside its transactions. The live replay buffer retains 4,096 events; use `session/transcript` for durable paging when the store supports it. Resume pauses restored queues until a client explicitly continues them.

Projects have stable host IDs and link registered user directories. `SpawnOptions.project_id` selects the linked folder and launch defaults, with an optional `project_revision` guard. Omit both Project and cwd to use the node owner's saved project-free directory; an explicit cwd selects that directory directly. Each session retains its original selection across Project/default edits and restart. Several sessions can share one folder without automatic subfolders or worktrees. Missing or replaced directories block execution while history remains readable. These continuity checks do not change native execution permissions. See [Project and directory selection](protocol.md#project-and-directory-selection).

`SpawnOptions.execution` supports per-session environment variables, a provider configuration directory, typed MCP servers, explicit host tool grants, tool policy, ephemeral sessions, and output schemas. Each adapter accepts supported native options and rejects unsupported ones. Environment and MCP credentials are removed from snapshots. Context-bound host tools use a private per-connection MCP channel with schema validation and turn cancellation; see [tool delivery](extension-api.md#agent-facing-tool-delivery). Live forks inherit their source's private execution declaration; `resume_with`, `fork_with`, and `fork_at_with` explicitly replace that declaration when new credentials or restarted sessions require it. `Runtime::generate` runs an isolated ephemeral session with tools disabled; providers unable to guarantee that policy return `Unsupported`.

`Runtime::account_quotas(AccountQuery)` reads native Codex or Claude account limits without opening a conversation or saving a session. `account_quotas_all` queries up to 32 explicit accounts, four at a time, and returns each account's available, unsupported, or error result in input order. Account execution accepts private environment overrides and a configuration home. Providers and logins without native quota facts return `Unsupported`.

## Daemon

```sh
./target/debug/swarmie serve
./target/debug/swarmie serve --websocket 127.0.0.1:8090
SWARMIE_TOKEN=your-token ./target/debug/swarmie serve --websocket 0.0.0.0:8090
SWARMIE_TOKEN=your-token ./target/debug/swarmie call ws://127.0.0.1:8090 runtime/discover
```

WebSocket clients use `Authorization: Bearer TOKEN` when configured. Non-loopback listeners require a token. All connected clients receive events for the daemon's sessions. Sessions survive client disconnects and close on daemon shutdown. Both transports use the same dispatcher and domain API. See [runtime protocol](protocol.md).

## Extend

Implement the public `Harness` and `AgentBackend` traits in an external crate, then register it with `Runtime::builder().harness(...)`. Implement `RuntimeExtension` for spawn/prompt middleware, ordered event observation, and permission policy hooks. Built-in adapters use those same public ports. See [extension API](extension-api.md) and the runnable [external harness example](../../examples/external-harness/src/main.rs).

The optional [permission policy extension](../extensions/permissions.md) provides Ask, AutoApprove, AcceptEdits, and Deny through `serve --permission-policy ask` and runtime commands. Recognized edit requests can be approved for one operation; questions, plans, and ambiguous requests remain interactive. Native provider sandbox settings remain separate.

[TypeScript, Python, Go, and Swift clients](../sdk/README.md) provide generated protocol contracts, HTTP access, and durable event iteration. TypeScript and Python can also own a stdio daemon. Local packaging is available; published registries and signed cross-platform installers are separate release work.

```sh
cargo run -p external-harness-demo
```

## Verify

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
npm ci --prefix tests/fixtures/harnesses
```

Offline tests cover the domain, native wire fixtures, ordering under backpressure, permission validation, lifecycle interruption, process cleanup, persistence, and actual stdio/WebSocket clients. Fixtures install Pi, OpenCode, and Qwen inside `tests/fixtures/harnesses`; their binaries are not included in version control.

For real model tests, see [live harness tests](../testing/README.md). The optional local inference proxy converts Anthropic Messages to a compatible Chat Completions endpoint for testing Claude's real native runtime. It belongs to test fixtures and is not part of the product runtime.

Cross-provider conversation transfer and dynamic native plugin loading remain outside the current surface. Remote clients use the same daemon protocol; the CLI and desktop support configured SSH access through an owned stdio byte proxy, with reconnection supervised by the native client. Saved connections pin the retained Environment/store/owner identity, and CLI calls guard the initialized runtime incarnation before dispatch. A different Environment at the same endpoint requires deliberate pairing; see [nodes and Environments](nodes-and-environments.md). Unsupported provider operations return explicit errors.

The desktop automatically attaches to and records **This computer**. Its titlebar selects a registered node for viewing and new work. Settings → Nodes adds prepared endpoints, retries or disconnects observation, and removes individual routes without stopping execution. Remote operations use a verified local gateway; guided SSH preparation remains pending. Saved HTTP, WebSocket, and SSH routes share a SQLite node registry inside the configured application root, with credentials stored separately. Multiple routes to one verified Environment share one node. Inspect the retained catalog without a liveness probe:

```sh
swarmie node list
```

The current development schema is version 3; an older internal schema requires a fresh development root and remains untouched. Client state and browser caches are scoped by retained node identity. The multi-node desktop selector, host routing, and remote preparation workflow are subsequent work.

## Native desktop and Git extension

The [desktop workspace](../desktop/README.md) uses Native SDK with a system WebView, assistant-ui, and shared HeroUI primitives and Tailwind layout. It opens repositories, creates worktrees, runs agent sessions, and reviews changes in a dark, rounded interface. Git is an optional extension in `extensions/git`; the runtime core has no Git dependency. Start its daemon with:

```sh
cargo run -p app -- serve --http 127.0.0.1:7447 \
  --desktop-assets apps/desktop/dist \
  --git-worktrees "$HOME/.local/state/swarmie/worktrees"
```
