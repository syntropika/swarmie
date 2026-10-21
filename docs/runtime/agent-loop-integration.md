# Agent Loop integration contracts

The Agent Loop consumes SessionStore, DurableTasks and neutral Model Contracts through Tembra
References and Registries. Providers remain independently replaceable. The default executable
catalog exposes the loop and SQLite SessionStore as optional compiled plugins. A Profile must
select their dependencies and a model-provider contribution; the default foundation does not
implicitly start inference. Runtime bootstrap registers the known hook codecs on its configured
Tembra JavaScript Executor before handing it to PluginManager. PluginManager imports no agent
contracts and remains outside ordinary model/tool dispatch.

## Session content and receipt conventions

Version-one `swarmie.message` contains the Session `Message` envelope. Its `content` can be a text
string or the canonical Model Contracts content-part array. User/assistant/tool messages retain
model tool-call IDs. Unknown roles are not interpreted by the default context adapter. Session
projections remain policy-free; context transformation belongs to the loop and contributing hooks.

| Event kind | Meaning |
| --- | --- |
| `swarmie.agent.turn-started` | Admitted StartTurn input |
| `swarmie.agent.model-request` | Prepared request after context and before-call hooks |
| `swarmie.agent.model-actual` | Effective request entering the managed model terminal |
| `swarmie.agent.model-started` | Durable intention before provider admission |
| `swarmie.agent.model-provider-confirmed` | Physical provider outcome before middleware result transformation |
| `swarmie.agent.model-confirmed` | Effective complete result after model middleware |
| `swarmie.agent.tool-intent` | Transformed arguments, persisted replay policy and optional veto |
| `swarmie.agent.tool-started` | Durable intention before managed tool admission |
| `swarmie.agent.tool-confirmed` | Confirmed execution outcome, denial or explicit reconciliation |
| `swarmie.agent.step-ended` | Fully processed model/tool step and accumulated admitted usage |
| `swarmie.agent.phase-error` | Sanitized phase/reason diagnostic; it does not assert effect rollback |
| `swarmie.agent.cancellation-requested` | Cancellation requested through the agent control surface |
| `swarmie.agent.turn-ended` | Loop decision, which can precede the task's actual terminal commit |
| `swarmie.agent.task-settled` | Reconciled authoritative terminal TaskStorage outcome |
| `swarmie.agent.operation-resolution` | Validated reference to evidence resolving an uncertain operation |

All kinds currently use payload version one. A provider receipt and effective middleware receipt
have separate immutable identities. If middleware fails after the provider result commits, a retry
can reenter the middleware while the model terminal reuses the physical receipt. The external
request is not repeated. A middleware result that bypasses the terminal still passes correlation,
role and persistence checks. Observers receive the effective result and, when entered, the actual
terminal request.

Loop-owned event and batch identities use SHA-256 over turn identity and semantic phase suffix.
The loop never places provider tool-call IDs directly into a TaskStorage identity. Envelopes
carry turn, task and step correlation; confirmed operation payloads additionally name the stable
operation ID. Cancellation request identity is independent of the current step. Append retries
keep the complete original payload, version, correlation and expected-head policy unchanged.

## Default checkpoint version one

A replacement handler registers task kind `swarmie.agent-turn`, definition version one, only if it
understands the persisted format and phase being resumed. The input is the public StartTurn DTO.
The checkpoint object has these exact fields:

| Field | Type and purpose |
| --- | --- |
| `step` | u32 model-step index, starting at zero |
| `tool_index` | u32 index within the confirmed model response |
| `tool_calls` | u32 accumulated admitted tool calls |
| `output_tokens` | Portable u64 counter capped to the turn budget |
| `started_at_ms` | Wall-clock deadline origin, including downtime |
| `input_sequence` | Committed user-message source position |
| `request_event` | Optional prepared-model-request event ID |
| `response_event` | Optional effective-model-result event ID |
| `tool_event` | Optional tool-intent event ID |
| `ending` | Optional controlled failure reason for finalization |

Phases are `start`, `prepare-model`, `model`, `model-result`, `prepare-tool`, `tool`, `tool-result`,
`step-end` and `finish`. Every checkpoint field is required; nullable fields explicitly use null.
No content, arguments, model response or tool result is copied into this checkpoint. A compatible
replacement may support a narrower set of phases and explicitly refuse others. Different formats
must select a new definition version and implement the Durable Tasks migration contract, with
explicit acceptance or rejection. Native code replacement uses a fresh Host and preserved stores.

## Model qualification and external effects

Inference messages, content parts, finish reasons, usage, capabilities and sanitized provider
failures come from Model Contracts, without importing a provider SDK. Streaming uses its managed
generation Contract and bounded single-consumer event window. Incomplete tool snapshots are
observations only. The loop retains the generation Reference until physical local settlement,
acknowledges processed event prefixes and treats the final response as authoritative.

Provider connection discovery must be unique. Unsupported effective capabilities fail before
submission; a capability marked model-dependent still requires remote acceptance. Unknown model
attempts park because remote billing or computation may already have occurred. External model
reconciliation references a correlated `model-provider-confirmed` event; tool reconciliation
references `tool-confirmed`. The operation must match the currently parked durable phase. The
loop does not offer an automatic unsafe replay permission.

The executable examples and agent acceptance suite use a synthetic provider implementing the
actual neutral Contracts. A concrete network adapter must additionally qualify credentials,
provider-specific continuation, usage omissions, streaming failure and remote cancellation limits.
Authorized opaque continuation extensions round-trip; the generic loop excludes hidden reasoning
prose. That exclusion is an explicit support constraint when an adapter requires other signed or
reasoning forms. It must be resolved through the owning Model Contract rather than a duplicate API.

Durable Tasks lifecycle notifications are independent observations. This loop does not subscribe
to them to decide execution progress or reconstruct conversation history. TaskStorage and Session
commits are reconciled by operation IDs and confirmed receipts, without assuming atomicity or
requiring an unfinished transactional outbox. Generic terminal inspection can reconcile current
outcome; it cannot manufacture missing intermediate task transitions.

## Executable examples

Both examples use a real Host, the existing scheduler and two independently persisted SQLite
stores. Their scripted model and functional numeric tool are test fixtures. Temporary example
storage belongs to the example process.

```sh
just rust-run -p swarmie-agent-loop-fixture --example turn -- simple
just rust-run -p swarmie-agent-loop-fixture --example turn -- tool
```
