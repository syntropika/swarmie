# mpai

A headless Rust runtime for installed coding agents. Use the Rust API, an interactive CLI, or a persistent JSON-RPC daemon. Harness integrations use native structured protocols; client code consumes one ordered stream of runtime events.

## Start

Requires Rust 1.88 or later. Install the harness you want to use and authenticate it with its own CLI, or configure a compatible inference endpoint.

```sh
cargo build --workspace
./target/debug/mpai discover
./target/debug/mpai probe codex
./target/debug/mpai quotas codex .
./target/debug/mpai run codex . --prompt "Explain this project"
./target/debug/mpai run pi .
./target/debug/mpai run claude . --prompt "Review this project"
./target/debug/mpai run opencode . --model provider/model
```

`run` without `--prompt` is interactive. Supported commands are `/steer TEXT`, `/cancel`, `/quit`, and `/answer OPTION [JSON]` for a pending request. Ordinary text starts a turn. A prompt during an active turn returns `Busy`; steering must be explicit and supported by the harness.

One-shot runs default to denying requests that would require interactive input. `--permissions approve` approves offered approval options; structured questions still require an answer through an interactive client or the daemon. `--json` prints normalized events as JSONL. `--timeout` bounds a one-shot turn and defaults to 600 seconds.

## Harnesses

| Harness | Structured interface | Resume | Fork | Steering | Permission requests |
| --- | --- | --- | --- | --- | --- |
| Codex | App Server JSON-RPC | Yes | Yes | Yes | Yes |
| Pi | RPC, version 1.0+ | Yes | Yes | Yes | Native extension dialogs |
| Claude Code | SDK control protocol and stream-json | Yes | Yes | Unsupported | Yes |
| OpenCode | HTTP API and SSE | Yes | Yes | Unsupported | Yes |
| Antigravity | Native stream-json | Yes | Unsupported | Unsupported | Native auto-execution |
| Generic ACP | ACP v1 JSON-RPC | Negotiated | Unsupported | Unsupported | Yes |

Gemini, Qwen, Cursor, Grok, Hermes, and Kiro are registered through ACP. Capabilities are negotiated with the installed version; a catalog entry does not establish authentication or successful model execution. A generic ACP command needs no catalog entry:

```sh
./target/debug/mpai run custom . --executable /path/to/agent --agent-arg=--acp
./target/debug/mpai serve --acp custom=/path/to/agent --acp-arg=--acp
```

Discovery is passive and reports executable availability. It does not claim authentication or model readiness. Session capabilities reflect adapter support and, for ACP, the handshake. Unsupported operations return an explicit error.

Native executable overrides and launch arguments are available through `--config`:

```sh
./target/debug/mpai run pi . --config '{"executable":"/path/to/pi"}'
```

`config.args` replaces the native base launch arguments; Pi and Claude still append runtime session/resume/model flags. Codex supports `approvalPolicy`, `sandbox`, `modelProvider`, `baseInstructions`, `developerInstructions`, and native `config` at spawn. OpenCode accepts an existing server through `config.url`, with optional `username` and `password`; otherwise mpai starts a supervised loopback server with a private credential. Use environment-based provider credentials rather than placing secrets in spawn configuration, which is saved in session snapshots.

Pi 1.0+ ends turns at `agent_settled`, including retries and queued work. For a deliberately configured older Pi installation, `config.legacyAgentEnd=true` selects its older completion event; that mode does not provide the newer settled semantics. Blocking Pi extension dialogs are normalized into permission or question requests, with native response translation.

Native JSON process wires and OpenCode HTTP/SSE accept `config.diagnostics` with optional `stallTimeoutMs` and `recording: {"path":"/absolute/trace.jsonl","maxBytes":8388608}`. Recordings redact known credentials and use a bounded, fresh file; opening or filling the trace emits a diagnostic and leaves the connection usable. Stall warnings apply only while an operation or turn is active and never cancel it. OpenCode records query-free request paths, response bodies, and parsed SSE events, without authentication headers or URL credentials.

## Rust API

```rust,no_run
use futures_util::StreamExt;
use mpai::{EventKind, SpawnOptions};

# async fn example() -> mpai::Result<()> {
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

Store session references durably with `storage::FileSessionStore`, or provide your own `SessionStore`. The default Rust builder uses `MemoryStore`. The CLI defaults to `$XDG_STATE_HOME/mpai/sessions` or `~/.local/state/mpai/sessions`; override it with `--state-dir`.

```sh
./target/debug/mpai sessions
./target/debug/mpai run codex . --resume RUNTIME_SESSION_UUID --prompt "Continue"
./target/debug/mpai run codex . --fork RUNTIME_SESSION_UUID --prompt "Explore another approach"
```

Snapshots contain the runtime ID, native provider reference, redacted options, capabilities, sequence, metadata, queue, and lifecycle state. The provider owns its native conversation. `FileSessionStore` also maintains an ordered runtime event journal for transcript replay across restarts. The live replay buffer retains 4,096 events; use `session/transcript` for durable paging when the store supports it. Resume pauses restored queues until a client explicitly continues them.

`SpawnOptions.execution` supports per-session environment variables, a provider configuration directory, typed MCP servers, tool policy, ephemeral sessions, and output schemas. Each adapter accepts supported native options and rejects unsupported ones. Environment and MCP credentials are removed from snapshots. Live forks inherit their source's private execution declaration; `resume_with`, `fork_with`, and `fork_at_with` explicitly replace that declaration when new credentials or restarted sessions require it. `Runtime::generate` runs an isolated ephemeral session with tools disabled; providers unable to guarantee that policy return `Unsupported`.

`Runtime::account_quotas(AccountQuery)` reads native Codex or Claude account limits without opening a conversation or saving a session. `account_quotas_all` queries up to 32 explicit accounts, four at a time, and returns each account's available, unsupported, or error result in input order. Account execution accepts private environment overrides and a configuration home. Providers and logins without native quota facts return `Unsupported`.

## Daemon

```sh
./target/debug/mpai serve
./target/debug/mpai serve --websocket 127.0.0.1:8090
MPAI_TOKEN=your-token ./target/debug/mpai serve --websocket 0.0.0.0:8090
MPAI_TOKEN=your-token ./target/debug/mpai call ws://127.0.0.1:8090 runtime/discover
```

WebSocket clients use `Authorization: Bearer TOKEN` when configured. Non-loopback listeners require a token. All connected clients receive events for the daemon's sessions. Sessions survive client disconnects and close on daemon shutdown. Both transports use the same dispatcher and domain API. See [runtime protocol](docs/protocol.md).

## Extend

Implement the public `Harness` and `AgentBackend` traits in an external crate, then register it with `Runtime::builder().harness(...)`. Implement `RuntimeExtension` for spawn/prompt middleware, ordered event observation, and permission policy hooks. Built-in adapters use those same public ports. See [extension API](docs/extension-api.md) and the runnable [external harness example](examples/external-harness/src/main.rs).

The optional [permission policy extension](extensions/permissions/README.md) provides Ask, AutoApprove, AcceptEdits, and Deny through `serve --permission-policy ask` and runtime commands. Recognized edit requests can be approved for one operation; questions, plans, and ambiguous requests remain interactive. Native provider sandbox settings remain separate.

[TypeScript, Python, Go, and Swift clients](packages/clients/README.md) provide generated protocol contracts, HTTP access, and durable event iteration. TypeScript and Python can also own a stdio daemon. Local packaging is available; published registries and signed cross-platform installers are separate release work.

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

For real model tests, see [live harness tests](tests/fixtures/README.md). The optional local inference proxy converts Anthropic Messages to a compatible Chat Completions endpoint for testing Claude's real native runtime. It belongs to test fixtures and is not part of the product runtime.

Cross-provider conversation transfer and dynamic native plugin loading remain outside the current surface. Remote clients use the same daemon protocol; the CLI supports authenticated SSH forwarding with optional supervised reconnection. Unsupported provider operations return explicit errors.

## Native desktop and Git extension

The [desktop workspace](apps/desktop/README.md) uses Native SDK with a system WebView, assistant-ui, and shared Radix/Tailwind primitives. It opens repositories, creates worktrees, runs agent sessions, and reviews changes in a dark, rounded interface. Git is an optional extension in `extensions/git`; the runtime core has no Git dependency. Start its daemon with:

```sh
cargo run -p app -- serve --http 127.0.0.1:7447 \
  --desktop-assets apps/desktop/dist \
  --git-worktrees "$HOME/.local/state/mpai/worktrees"
```
