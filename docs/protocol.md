# Runtime protocol v1

JSON-RPC 2.0 runs over newline-delimited UTF-8 stdio, text WebSocket frames, or HTTP requests. Requests use string or numeric IDs, notifications omit `id`, and batches are supported. Parameters are named JSON objects. Responses preserve request IDs and notifications never receive a response. Stdout contains only protocol JSON; diagnostics go to stderr.

Initialize:

```json
{"jsonrpc":"2.0","id":1,"method":"runtime/initialize"}
```

Create and prompt:

```json
{"jsonrpc":"2.0","id":2,"method":"session/create","params":{"harness":"codex","cwd":"/absolute/project/path","model":null,"config":{}}}
{"jsonrpc":"2.0","id":3,"method":"session/prompt","params":{"sessionId":"session-uuid","text":"Explain this project"}}
```

Create returns the session snapshot, including `id`, `native`, `capabilities`, `state`, and `sequence`. Prompt returns `{"accepted":true}`; completion arrives as an event.

| Method | Parameters | Result |
| --- | --- | --- |
| `runtime/initialize` | none | Protocol information and supported methods |
| `runtime/schema` | none | Generated JSON Schema, domain types, and method contracts |
| `runtime/discover` | none | Passive harness discovery |
| `runtime/probe` | `harness` | Active native version and readiness diagnostics |
| `runtime/quotas` | `harness`, `cwd`, optional `config`, `execution` | Native account facts without a conversation or runtime session |
| `runtime/quotas/all` | `queries`: 1–32 account queries | Ordered per-account `available`, `unsupported`, or `error` results |
| `runtime/generate` | Spawn fields, prompt, optional `timeoutMs` | Text or serialized structured result from an isolated, tools-disabled ephemeral session |
| `session/create`, `agent/spawn` | `harness`, `cwd`, optional `model`, `config`, `execution` | Snapshot |
| `session/list` | none | Saved snapshots |
| `session/resume` | `sessionId`, optional `execution` | Open snapshot; explicit execution overrides require a closed session |
| `session/fork` | `sessionId`, optional `messageId`, `execution` | Independent snapshot; message anchors must be completed, retained native assistant messages. Explicit execution replaces the child's full declaration |
| `session/get` | `sessionId` | Open snapshot |
| `session/metadata` | `sessionId`, optional `title`, `archived` | Updated snapshot |
| `session/events` | `sessionId`, optional `afterSequence` | Retained live events after that sequence |
| `session/transcript` | `sessionId`, optional `afterSequence`, `limit` | Durable ordered event page when supported by the store |
| `session/queue` | `sessionId`, prompt fields | Persisted queue item |
| `session/queue/list` | `sessionId` | Queue and pause state |
| `session/queue/update` | `sessionId`, `id`, prompt fields | null |
| `session/queue/remove` | `sessionId`, `id` | null |
| `session/queue/pause` | `sessionId`, `paused` | null |
| `session/prompt`, `session/steer` | `sessionId`, `text` or `content` | Acceptance |
| `session/cancel` | `sessionId` | null |
| `permission/respond` | `sessionId`, `id`, `option`, optional `answers`, `data` | null |
| `agent/configure` | `sessionId`, `config` | Native configuration result |
| `agent/models` | `sessionId` | Native model catalog |
| `agent/configuration` | `sessionId` | Native configuration descriptors |
| `agent/commands`, `agent/quotas` | `sessionId` | Native catalog/account facts, or `Unsupported` |
| `session/rollback` | `sessionId`, positive `turns`, optional `scope` | null; defaults to `conversationOnly`, file restoration requires explicit `conversationAndFiles` support |
| `session/compact` | `sessionId` | null |
| `session/close`, `agent/kill` | `sessionId` | null |

Content supports `{"type":"text","data":"prompt"}`, `{"type":"image","data":{"data":"base64","mime_type":"image/png"}}`, and `{"type":"resource","data":{"uri":"file:///path"}}`. Adapters reject content they do not support; `capabilities.images` identifies image support.

Events:

```json
{"jsonrpc":"2.0","method":"session/event","params":{"sessionId":"session-uuid","event":{"sessionId":"session-uuid","sequence":7,"turnId":"turn-uuid","source":"codex","timestampMs":1700000000000,"type":"text.delta","data":{"text":"Hello","item_id":"native-item"},"raw":{"method":"item/agentMessage/delta","params":{"delta":"Hello"}}}}}
```

The event envelope contains a monotonic sequence per session, timestamp, optional turn ID, source, semantic type/data, and optional raw JSON. Semantic types cover sessions, turns, user/assistant messages, text/reasoning deltas, tools, permissions, changes/diffs, plans, subagents, usage, state, errors, and raw provider facts. `session.ended` is the last event in a logical session's stream. A native `model.rerouted` event carries `fromModel`, `toModel`, and an optional `reason`; the actual model for that turn does not change the session's requested model.

Tool data has common `id`, optional `name`, `status`, `input`, and `output` fields. Sparse updates preserve earlier identity and input. `metadata.outputDelta` appends to earlier output; a non-null full output replaces it. Native failed, denied, and cancelled outcomes remain distinct. ACP display content remains in metadata and supplies tool output when native raw output is empty. Usage data has `scope`, optional `inputTokens`, `outputTokens`, `cachedInputTokens`, `cacheWriteTokens`, `totalTokens`, and `costUsd`. Per-model-call counters use `scope: "call"`; they do not claim cumulative turn totals. Missing counters are null rather than invented. Native details remain in `raw`. Clients can render those common fields without parsing a harness protocol.

Permission requests provide an `id`, kind/title, optional command/paths, offered options, and provider metadata. Respond with an exact option ID. Duplicate/unknown responses and unoffered options are rejected. Typed `questions` contain the stable ID, prompt, options, multiplicity, free-text permission, and secret flag. Respond with top-level `answers` keyed by question ID: a string for a single answer, a string array for multiple selections. Legacy `metadata.questions` and `data.answers` remain accepted; conflicting duplicate forms are rejected. The adapter translates to the native wire shape. Generic provider-specific input remains in metadata and raw events.

Tool events optionally expose typed `details` for native kind/title, command/cwd, file locations, diffs, progress, and child agent information. Plans have typed entries and optional text/markdown/status. Subagent updates expose optional identity, parent relationships, model, progress, and usage. Fields are present only when the native event supplies them; provider extras and raw receipts remain available. Older stored plan/subagent payloads remain readable.

Account queries accept only environment overrides and a configuration home in `execution`; they cannot declare MCP servers, tools, output schemas, or session options. Environment values are input-only. Codex reads its native account rate limits after connection initialization without creating a thread. Claude uses a nonpersisted, bare native control connection and submits no user message. Each query has a 60-second deadline; the aggregate runs at most four queries concurrently, preserves input order, and retains unsupported and failed accounts independently.

Offered permission option kinds distinguish one operation (`approve_once`, `deny_once`) from session scope (`approve_session`, `deny_session`). Unknown native options remain unknown. Optional denial text uses `data.reason` or `data.message` where the provider accepts it. Cancelling a question is distinct from a terminal turn outcome; clients wait for the actual native turn boundary.

Background requests can arrive after the parent turn ends. They carry no parent `turnId`, remain answerable through `permission/respond`, and place the session in `needs_input`. New prompts, queue promotion, fork, rollback, and compaction wait for resolution. Resolving the last background request returns an otherwise idle session to `idle` without creating a turn or emitting another `turn.ended`.

Error codes include JSON-RPC parse/invalid/method/parameter errors (`-32700`, `-32600`, `-32601`, `-32602`) and runtime errors: provider/storage/timeout (`-32000`), unsupported capability (`-32001`), busy (`-32002`), closed (`-32003`). The terminal reason determines turn outcome; recoverable provider diagnostics may precede a successful terminal event.

A lagging connection receives `runtime/lagged` with `missed` and `recovery: "session/events"`. Consumers should resume from their last processed sequence. If that sequence is older than retained history, live replay returns an explicit error. `session/transcript` pages the canonical stored journal across restarts when `runtime/initialize.transcriptDurability` is available. An interrupted stored turn receives one terminal recovery boundary before new work is accepted.

WebSocket sessions remain open when a client disconnects. `Authorization: Bearer TOKEN` is checked during handshake when the daemon is configured with a token. A non-loopback listener requires a token. All authorized clients share the daemon's session namespace and receive its global event notifications.

## Extension commands and native HTTP clients

`extension/list` returns installed extension IDs with their advertised commands. `extension/invoke` accepts `{ "extensionId": "git", "command": "project.open", "arguments": { "path": "/path/to/repository" } }`. Commands are also callable through the in-process runtime API. Disabled extensions reject invocation.

`mpai serve --permission-policy ask` registers the optional `permissions` extension. Its `policy.get` and `policy.set` commands read or select `ask`, `auto_approve`, `accept_edits`, or `deny`. Policies apply to future requests. Questions, plans, unknown operations, and ambiguous options remain interactive; automatic approval or denial selects a recognized offered option for one operation. The policy does not alter a provider's native sandbox or constrain providers that do not emit permission requests. See [permission policy extension](../extensions/permissions/README.md).

`mpai serve --http 127.0.0.1:7447 --git-worktrees DIR` serves the same JSON-RPC dispatcher at `POST /rpc`. Notifications return HTTP 204; requests and batches return JSON. HTTP uses the same bearer token policy as WebSocket and requires a token for non-loopback binding. Browser Origin headers are rejected by default. `--desktop-assets DIR` enables a frontend on a loopback listener and accepts only its exact origin and Host for RPC; it does not enable CORS for other origins. The HTTP transport does not push session notifications.

`session/events` accepts an optional `limit` from 1 to 4096. Each response contains the next ordered events after `afterSequence`, up to that limit. Advance the cursor only through received events. Retained-history gaps remain explicit errors; paging does not extend the 4096-event live retention window.

The maintained [TypeScript, Python, Go, and Swift clients](../packages/clients/README.md) use durable transcript polling. TypeScript and Python also support an owned stdio daemon. Their generated contracts come from `runtime/schema`.
