# Runtime protocol v1

JSON-RPC 2.0 runs over newline-delimited UTF-8 stdio, text WebSocket frames, or HTTP requests. Requests use string or numeric IDs, notifications omit `id`, and batches are supported. Parameters are named JSON objects. Responses preserve request IDs and notifications never receive a response. Stdout contains only protocol JSON; diagnostics go to stderr.

Initialize:

```json
{"jsonrpc":"2.0","id":1,"method":"runtime/initialize"}
```

Initialization returns `runtimeIdentity: {identity: {environmentId, storeId, ownerScopeId}, incarnationId}`. The identity is retained across restarts; the incarnation identifies one live execution owner. Saved remote CLI connections and the desktop connection manager pin the retained identity and include the freshly initialized value as an optional top-level `expectedRuntime` on each call. The receiver rejects a different identity or incarnation before dispatch with error `-32005`, `data.code: "runtime_target_mismatch"`, and `data.effectKnowledge: "none"`. Malformed targets return `-32602`. Each batch item has its own target; rejected notifications produce no response or method effect. Other clients must explicitly use the guard to obtain this protection. `runtime/schema` publishes its input schema under `x-requestTarget`. See [nodes and Environments](nodes-and-environments.md).

`runtime/shutdown` takes `{}` and returns null after signalling an explicit owner stop. HTTP drains accepted requests before closing; stdio and WebSocket transports also observe the shared stop signal. Native processes are then shut down and retained history remains available. The acknowledgement is a stop request, not proof that cleanup has already finished. Use a freshly verified `expectedRuntime` to avoid stopping a replacement owner. The local `runtime stop` CLI additionally waits for listener closure and the configured owner lock's release. Disconnecting an HTTP or WebSocket client does not invoke this operation.


Create and prompt:

```json
{"jsonrpc":"2.0","id":2,"method":"session/create","params":{"harness":"codex","cwd":"/absolute/project/path","model":null,"config":{}}}
{"jsonrpc":"2.0","id":3,"method":"session/prompt","params":{"sessionId":"session-uuid","idempotencyKey":"unique-persisted-request-key","text":"Explain this project"}}
```

Create returns the session snapshot, including `id`, `native`, `capabilities`, `state`, and `sequence`. Prompt returns a `SubmissionReceipt` with a stable submission ID, request key, turn ID, delivery knowledge, recovery requirement, optional native outcome, and optional error. Completion also arrives as an event.

| Method | Parameters | Result |
| --- | --- | --- |
| `runtime/initialize` | none | Protocol information, supported methods, and `runtimeIdentity` |
| `runtime/schema` | none | Generated JSON Schema, domain types, and method contracts |
| `mcp/list` | Optional harness, directory, configuration home or native session | Node-local desired configuration and qualified native inventory |
| `mcp/mutate` | Action, query and definition or exact entry identity/revision | Saved configuration, affected session IDs and optional native sign-in URL |
| `mcp/reopen` | `sessionId` | Reopened idle native binding with current MCP settings |
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
| `session/prompt` | `sessionId`, `idempotencyKey`, `text` or `content` | Retained submission receipt |
| `session/submission` | `sessionId`, `idempotencyKey` | Receipt or null, without native launch/resume |
| `session/submission/reconcile` | `sessionId`, `idempotencyKey` | Receipt or null, after inspecting an already open native connection |
| `session/steer` | `sessionId`, `text` or `content` | Acceptance |
| `session/cancel` | `sessionId` | null |
| `permission/respond` | `sessionId`, `id`, `option`, optional `answers`, `data` | null |
| `agent/configure` | `sessionId`, `config` | Configuration receipt with values, descriptors and execution policy |
| `agent/models` | `sessionId` | Native model catalog |
| `agent/configuration` | `sessionId` | Native configuration descriptors |
| `agent/commands`, `agent/quotas` | `sessionId` | Native catalog/account facts, or `Unsupported` |
| `session/rollback` | `sessionId`, positive `turns`, optional `scope` | null; defaults to `conversationOnly`, file restoration requires explicit `conversationAndFiles` support |
| `session/compact` | `sessionId` | null |
| `session/close`, `agent/kill` | `sessionId` | null |

## Prompt submission and recovery

Persist a unique request key before each intentional prompt. Keys contain 1–128 bytes without control characters; `queue:` is reserved for runtime queue delivery. Keys belong to one personal-node session. JSON-RPC IDs identify transport responses and have no deduplication role. Equivalent text/content prompt representations compare as the same authored content. Repeating a retained key with the same content returns its receipt before current busy checks; changing content returns RPC error `-32004` with `data.code: "idempotency_conflict"`. Identical content with a new key is a separate intended submission.

Delivery knowledge is independent of execution outcome:

| Delivery | Evidence | Behavior |
| --- | --- | --- |
| `not_sent` | Recorded intent; no native call made | Recovery retains the input and blocks automatic dispatch |
| `in_flight` | Attempt marker committed before the native call | Duplicates return the existing attempt; recovery reads never dispatch |
| `accepted` | Native acceptance or qualified terminal evidence | Acceptance does not mean completion; retained outcomes report native completion separately |
| `rejected` | Adapter explicitly certifies no native effects with `Error::Rejected` | Same-key repeats retain the rejection; deliberate replacement uses a new key |
| `unknown` | Attempted delivery without sufficient acknowledgement | Pending work stays paused; no automatic native resend |

`session/submission` reads a retained key with an open or closed session and never creates/resumes a native conversation. Null means no retained receipt was found, not proof that an earlier request cannot still arrive. Following a transport failure, read the original key. An explicit same-key prompt is deduplicated by the serialized owner while its ledger is retained; clients never automatically resend after a timeout. Prompt failures carry `data.code`, `data.effectKnowledge`, and the original receipt query parameters. The failure's effect knowledge describes that invocation; it does not erase previously retained evidence.

The SQLite store commits private delivery records in `state/host.sqlite`, with WAL and full synchronization. Typed host transactions can group snapshot, submission and canonical event changes; queue and live metadata mutations commit with their events before publication. Intent and attempt marker precede provider work. Native calls do not span a storage transaction. Accepted receipts and terminal evidence are authoritative even if later snapshot/journal writes fail. Resume reconstructs missing start/user/terminal journal records from committed ledger evidence without repeating native execution. This repair can append recovered events later than their original occurrence; repaired timestamps describe recovery time. The ledger retains projected content and terminal native reason/raw evidence for this purpose. Receipts are retained independently of live event-history trimming; deleting or expiring a ledger removes its deduplication guarantee. The in-memory store retains these facts only for the lifetime of that store.

Restart converts an abandoned `in_flight` attempt to `unknown`. A known acceptance without a retained native outcome keeps `delivery: "accepted"` and sets `recoveryRequired: true`. Pending input survives, queues remain paused, and prompt/steer/rollback/compact/queue-resume cannot bypass the blocker. Generic provider errors do not certify rejection. An explicit native resume checks unresolved submissions against qualified native history before reconstructing the journal. A durable `not_sent` intent can be resolved as rejected without calling the provider because no native attempt followed that intent; its authored input remains retained for a deliberate new-key retry.

`session/submission/reconcile` accepts the same session/key parameters as `session/submission` and returns a receipt or null. It inspects an already open native connection; a closed session returns `Closed` and must be explicitly resumed first. This operation never submits a prompt. A missing native match is not proof of rejection. Unsupported adapters return `Unsupported`; ambiguous identities, incomplete pages, nonadvancing cursors and failed reads leave the original delivery evidence unresolved. Already settled receipts and absent ledger keys require no native read. The effective `submissionReconciliation` capability distinguishes qualified native lookup support from the runtime's universal retained-receipt lookup.

Qualified native terminal facts are committed to the delivery ledger before recovered assistant messages and terminal journal events are appended. Repeated recovery does not repeat native dispatch or duplicate those journal messages. Correlation uses the opaque submission identity committed before the native call, rather than authored text, transport IDs or timestamps. Acceptance alone preserves `recoveryRequired: true` until native completion is confirmed. Unresolved submissions do not receive a fabricated terminal outcome during restart.

The current Codex adapter sends that identity as `clientUserMessageId` and resolves an exact native user item's `clientId` through fully loaded, paginated turns. Completed and failed turns provide terminal facts. Interrupted history requires a native `completedAt` timestamp because history views may otherwise synthesize interruption from an abandoned active turn. Native partial or unfamiliar statuses establish acceptance only. Successful reconciliation restores native assistant messages and retains the entire native turn as raw evidence; it does not reconstruct every historical tool/progress/usage event. Ephemeral sessions have no recoverable native history. Other adapters currently retain unresolved evidence unless their own qualified native recovery is implemented. Active-turn reattachment and a user-authorized replacement of an unresolved attempt remain separate work. Queues stay paused after recovery until explicitly resumed.

The desktop's **Check request status** action explicitly reconnects the native session, reads qualified evidence and reloads the canonical journal. It preserves draft changes and explains unsupported or still unresolved recovery. Transport-only recovery continues to use `session/submission`, without opening a native session or resending the prompt.

The daemon and direct native-writing CLI commands take an OS owner lock for their state directory. The managed desktop serializes discovery/startup separately, attaches to a live owner, and leaves it running when the native client exits. Loopback HTTP owners publish a private `runtime/http-endpoint.json` discovery record; clients verify it against the retained store and live initialization response before attachment or explicit stop. Process exit releases it. Library embeddings must likewise provide one serialized runtime owner per store. The guarantee is one runtime dispatch attempt for a retained key; it does not certify the external provider's internal behavior. Durable idempotency is implemented for prompt submission and queued native dispatch, not yet for queue admission, steering, cancellation, permission responses, or general extension commands. Queued dispatch uses the queue item's retained identity; editing a previously rejected item creates a fresh identity. Collaboration actor scopes, binding-generation guards, receipt expiration, remain separate work.

## Project and directory selection

The host exposes `directory/register` (`{path}`), `directory/get` (`{id,revision}`), `project/create`, `project/list`, `project/get` (`{projectId}`), and `project/update`. Project creation accepts a name, optional directory reference and nonsecret `launch` defaults (`harness`, `model`, `config`). Updates require `projectId` and `expectedRevision`; optional fields are `name`, `archived`, `directory`, `unlinkDirectory` and `launch`. The launch object replaces the saved defaults. Linking and unlinking together are invalid. Archive/unlink never delete user directories or retained history.

`settings/directory` returns the node owner's current project-free default or null. `settings/setDirectory` takes `{expectedRevision,directory:{id,revision}}`; use revision zero only for the first selection. Stale revisions and references from another node fail explicitly.

For Project launches, pass `projectId` and optionally `projectRevision` to `session/create`. An accompanying cwd must identify the same directory. Without a Project or cwd, the host resolves its saved project-free default. A missing default is an error. Explicit cwd remains supported for CLI/library launchers and deliberately selected alternative worktrees. Project-free selection is captured at host preparation time; an unprepared client draft does not pin a prior host default.

Every runtime-created snapshot contains `workingContext`: context/session/binding IDs, Environment identity, the retained directory resource, and its selection source/revision. `session/context` takes `{sessionId}` and returns `{context,available,reason}` without connecting a Harness. Missing or replaced folders keep snapshots, metadata and transcripts readable while blocking execution. Resume and fork retain the original selection even after Project/default edits; fork receives new IDs. Current Project settings are not reapplied on resume.

`runtime/configuration` and `runtime/generate` use the same Project/default resolution rules. Preview does not persist a context or conversation. Generation retains no conversation in the host store. Directory registration may still persist an explicitly selected locator. Native-existing imports preserve their original cwd and provenance without starting a native process. An unverified missing native directory remains readable but is not automatically qualified for execution when a path reappears.

The directory checks are pre-operation continuity observations. They do not grant execution permissions or enforce filesystem isolation.

Content supports `{"type":"text","data":"prompt"}`, `{"type":"image","data":{"data":"base64","mime_type":"image/png"}}`, and `{"type":"resource","data":{"uri":"file:///path"}}`. Adapters reject content they do not support; `capabilities.images` identifies image support.

Capabilities are records rather than booleans. Each feature reports `availability` (`available`, `unavailable`, `unsupported`, or `unknown`), `source` (`harness`, `runtime`, or `extension`), and `evidence` (`declared` or `negotiated`), plus an optional explanatory `reason` and `sourceId`. Discovery can leave session-dependent features unknown. Effective session records supersede discovery declarations. Negotiated evidence describes native protocol facts, not live-task certification. Availability does not imply that an operation is currently admissible while busy or waiting for interaction. The queue is supplied by the runtime; an extension source does not promise an installed augmentation route.

`agent/models` returns typed model descriptors with `id`, `label`, optional `description` and `provider`, and optional native `metadata`. `agent/configuration` returns typed settings with an ID, label, `type` (`select`, `boolean`, `string`, or `number`), choices where applicable, and `application` (`creation_only`, `next_turn`, or `current_turn`). Missing `value` means unobserved; present null can acknowledge a reset. Provider-specific identifiers and metadata remain available. Similar names or choices do not establish equivalent model behavior across Harnesses.

`runtime/configuration` returns the effective capabilities of its temporary preview connection and nullable `models`/`configuration` catalogs. Null means unavailable; an empty array means an available, empty catalog. A Harness that requires a real session for discovery returns an explicit unsupported-preview error rather than creating a hidden persistent conversation.

`agent/configure` returns a typed receipt with confirmed `values`, current `configuration` descriptors, and optional native `raw` evidence. The runtime validates types, scopes and offered settings, while adapters validate model-dependent choices. Creation-only changes require a new session. Next-turn changes require idle; current-turn changes require an explicitly advertised descriptor. After native application, readback and acknowledged resets determine retained values. A rejected multi-setting update can still have applied earlier fields: observed changes are retained and published through `session.updated` before returning the original error. If readback fails, the request fails; no confirmed receipt or automatic retry guarantee is claimed. Optional unavailable catalogs return errors rather than pretending to be empty.

Events:

```json
{"jsonrpc":"2.0","method":"session/event","params":{"sessionId":"session-uuid","event":{"sessionId":"session-uuid","sequence":7,"turnId":"turn-uuid","source":"codex","timestampMs":1700000000000,"type":"text.delta","data":{"text":"Hello","item_id":"native-item"},"raw":{"method":"item/agentMessage/delta","params":{"delta":"Hello"}}}}}
```

For native conversation rollback, adapters resolve the requested count to the provider's actual history boundary. It does not restore files unless `conversationAndFiles` is explicitly requested and supported. Missing/ambiguous anchors, unfinished turns and nonadvancing history pagination fail before mutation. Native rollback can have provider-specific history limits and may be unavailable for ephemeral sessions.

The event envelope contains a monotonic sequence per session, timestamp, optional turn ID, source, semantic type/data, and optional raw JSON. Semantic types cover sessions, turns, user/assistant messages, text/reasoning deltas, tools, permissions, changes/diffs, plans, subagents, usage, state, errors, and raw provider facts. `session.ended` is the last event in a logical session's stream. A native `model.rerouted` event carries `fromModel`, `toModel`, and an optional `reason`; the actual model for that turn does not change the session's requested model.

`turn.ended.data.outcome` is one of `completed`, `cancelled`, `interrupted`, `failed`, `limited`, `refused`, `handled`, or `unknown`. These categories describe how execution ended; `completed` does not prove task achievement. `handled` means the input was handled without a model turn. Unknown native reasons remain `unknown`; limits and refusals are not successful completion. Optional `nativeReason` retains the original provider reason, and `raw` retains the native terminal evidence. Runtime-generated endings, including interrupted restart recovery, can omit `nativeReason`. Clients use `outcome` for behavior rather than translating provider strings.

Tool data has common `id`, optional `name`, `status`, `input`, and `output` fields. Sparse updates preserve earlier identity and input. `metadata.outputDelta` appends to earlier output; a non-null full output replaces it. Native failed, denied, and cancelled outcomes remain distinct. ACP display content remains in metadata and supplies tool output when native raw output is empty. Usage data has `scope`, optional `inputTokens`, `outputTokens`, `cachedInputTokens`, `cacheWriteTokens`, `totalTokens`, and `costUsd`. Per-model-call counters use `scope: "call"`; they do not claim cumulative turn totals. Missing counters are null rather than invented. Native details remain in `raw`. Clients can render those common fields without parsing a harness protocol.

Permission requests provide an `id`, kind/title, optional command/paths, offered options, and provider metadata. Respond with an exact option ID. Duplicate/unknown responses and unoffered options are rejected. Typed `questions` contain the stable ID, prompt, options, multiplicity, free-text permission, and secret flag. Respond with top-level `answers` keyed by question ID: a string for a single answer, a string array for multiple selections. Legacy `metadata.questions` and `data.answers` remain accepted; conflicting duplicate forms are rejected. The adapter translates to the native wire shape. Generic provider-specific input remains in metadata and raw events.

Tool events optionally expose typed `details` for native kind/title, command/cwd, file locations, diffs, progress, and child agent information. Plans have typed entries and optional text/markdown/status. Subagent updates expose optional identity, parent relationships, model, progress, and usage. Fields are present only when the native event supplies them; provider extras and raw receipts remain available. Older stored plan/subagent payloads remain readable.

Account queries accept only environment overrides and a configuration home in `execution`; they cannot declare MCP servers, tools, output schemas, or session options. Environment values are input-only. Codex reads its native account rate limits after connection initialization without creating a thread. Claude uses a nonpersisted, bare native control connection and submits no user message. Each query has a 60-second deadline; the aggregate runs at most four queries concurrently, preserves input order, and retains unsupported and failed accounts independently.

Offered permission option kinds distinguish one operation (`approve_once`, `deny_once`) from session scope (`approve_session`, `deny_session`). Unknown native options remain unknown. Optional denial text uses `data.reason` or `data.message` where the provider accepts it. Cancelling a question is distinct from a terminal turn outcome; clients wait for the actual native turn boundary.

Background requests can arrive after the parent turn ends. They carry no parent `turnId`, remain answerable through `permission/respond`, and place the session in `needs_input`. New prompts, queue promotion, fork, rollback, and compaction wait for resolution. Resolving the last background request returns an otherwise idle session to `idle` without creating a turn or emitting another `turn.ended`.

Error codes include JSON-RPC parse/invalid/method/parameter errors (`-32700`, `-32600`, `-32601`, `-32602`) and runtime errors: provider/storage/timeout (`-32000`), unsupported capability (`-32001`), busy (`-32002`), closed (`-32003`). The canonical terminal outcome determines the execution result; recoverable provider diagnostics may precede a successful terminal event.

A lagging connection receives `runtime/lagged` with `missed` and `recovery: "session/events"`. Consumers should resume from their last processed sequence. If that sequence is older than retained history, live replay returns an explicit error. `session/transcript` pages the canonical stored journal across restarts when `runtime/initialize.transcriptDurability` is available. A stored active turn without a corresponding submission ledger can receive a host interruption boundary. Unresolved ledger entries do not synthesize terminal outcomes and keep new work blocked until qualified evidence resolves them.

WebSocket sessions remain open when a client disconnects. `Authorization: Bearer TOKEN` is checked during handshake when the daemon is configured with a token. A non-loopback listener requires a token. All authorized clients share the daemon's session namespace and receive its global event notifications.

## Extension commands and native HTTP clients

`extension/list` returns installed extension IDs with their advertised commands. `extension/invoke` accepts `{ "extensionId": "git", "command": "project.open", "arguments": { "path": "/path/to/repository" } }`. Commands are also callable through the in-process runtime API. Disabled extensions reject invocation.

`swarmie serve --permission-policy ask` registers the optional `permissions` extension. Its `policy.get` and `policy.set` commands read or select `ask`, `auto_approve`, `accept_edits`, or `deny`. Policies apply to future requests. Questions, plans, unknown operations, and ambiguous options remain interactive; automatic approval or denial selects a recognized offered option for one operation. The policy does not alter a provider's native sandbox or constrain providers that do not emit permission requests. See [permission policy extension](../extensions/permissions/README.md).

`swarmie serve --http 127.0.0.1:7447 --git-worktrees DIR` serves the same JSON-RPC dispatcher at `POST /rpc`. Notifications return HTTP 204; requests and batches return JSON. HTTP uses the same bearer token policy as WebSocket and requires a token for non-loopback binding. Browser Origin headers are rejected by default. `--desktop-assets DIR` enables a frontend on a loopback listener and accepts only its exact origin and Host for RPC; it does not enable CORS for other origins. The HTTP transport does not push session notifications.

`session/events` accepts an optional `limit` from 1 to 4096. Each response contains the next ordered events after `afterSequence`, up to that limit. Advance the cursor only through received events. Retained-history gaps remain explicit errors; paging does not extend the 4096-event live retention window.

The maintained [TypeScript, Python, Go, and Swift clients](../sdk/README.md) use durable transcript polling. TypeScript and Python also support an owned stdio daemon. Their generated contracts come from `runtime/schema`.

## Execution profiles

Use `config: {"executionProfile":"ask"}`, `"review"` or `"full"` for a common selection. The server compiles supported equivalent mappings and rejects unavailable choices or a configure patch mixing common intent with its native controls. Session snapshots, configuration receipts and previews expose `executionPolicy`; `agent/configuration` includes an `executionProfile` descriptor whose `metadata.policy` contains the same contract. Clients must preserve the distinction between requested, staged, acknowledged and observed configuration. An explicit native control edit selects custom intent. `capabilities.operationApprovals` is separate from interactive question/request support. See [execution policy](execution-policy.md) for evidence, recovery and host authority rules.

## Scoped host tool protocol

`execution.hostTools` is an explicit list of grants. Each has `extensionId`, `toolId`, and optional `argumentConstraints`, mapping parameter names to allowed JSON values. For example:

```json
{"hostTools":[{"extensionId":"workspace","toolId":"read_file","argumentConstraints":{"path":["notes.txt"]}}]}
```

The host supplies the session, turn and retained directory. These selectors are excluded from model arguments. A grant does not alter native approval settings or authorize native tools. Host tool grants conflict with `execution.tools:"disabled"`. The HTTP runtime installs delivery automatically; a runtime without a supported delivery configuration rejects the selection.

The private `/tools/rpc` endpoint uses a session-scoped bearer channel supplied only to the native MCP process. It is distinct from general daemon authentication and rejects browser-origin requests. The channel cannot call general JSON-RPC methods. Requests use ordinary JSON-RPC IDs and the following methods:

| Method | Parameters | Result |
| --- | --- | --- |
| `tools/list` | `{}` | Granted MCP tool definitions |
| `tools/prepare` | `name`, `arguments` | Invocation receipt bound to the current owning turn |
| `tools/execute` | `invocationId` | Completed receipt, explicit origin and validated result |
| `tools/cancel` | `invocationId` | Current receipt after cancellation |
| `tools/status` | `invocationId` | Current live receipt |

Receipts contain `id`, `sessionId`, `turnId`, `bindingId`, `extensionId`, `toolId` and `state`. States are `prepared`, `running`, `completed`, `cancelled` and `failed`. An execution timeout is a failed invocation. An already completed effect is not reversed by cancellation. The live registry is bounded, completed receipts can expire, and a lost result does not authorize re-execution of the same invocation.

Scoped errors use `-32800` for cancellation and `-32001` for rejected or failed operations. Unknown or revoked channels receive HTTP 401. Successful MCP text results include the receipt, origin and payload. Execution errors include invocation identity and origin, with current state when it can still be queried; native approval rejection can occur before an invocation exists. Client cancellation notifications use `notifications/cancelled` with `params.requestId`; the bridge maps the native request to its prepared host invocation. General session cancellation also invalidates host invocations directly, independently of native notification delivery.

See [MCP management](mcp-management.md) for server origins, credential references, native scope rules and restart behavior.
