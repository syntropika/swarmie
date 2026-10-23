# Agent Loop integration contracts

The Agent Loop consumes SessionStore, DurableTasks and neutral Model Contracts through Tembra
References and Registries. Providers remain independently replaceable. The default executable
catalog exposes the loop and SQLite SessionStore as optional compiled plugins. A Profile must
select their dependencies and a model-provider contribution; the default foundation does not
implicitly start inference. Compiled Core Plugins declare their language adapters through `with_language_adapter`. Generic
Runtime initialization registers all adapters on its configured Tembra JavaScript Executor before
source/application installation and PluginManager cached recovery, preserving both task and agent
hook definitions. PluginManager imports no agent
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
| `swarmie.agent.turn-ended` | Loop decision, which can precede the task's actual terminal commit |
| `swarmie.agent.task-settled` | Reconciled authoritative terminal TaskStorage outcome |
| `swarmie.agent.operation-resolution` | Validated reference to evidence resolving an uncertain operation |

Agent event kinds use payload version two; the independent `swarmie.message` envelope retains
Session payload version one. Agent payloads include the new conversation scope where applicable. A provider receipt and effective middleware receipt
have separate immutable identities. If middleware fails after the provider result commits, a retry
can reenter the middleware while the model terminal reuses the physical receipt. The external
request is not repeated. A middleware result that bypasses the terminal still passes correlation,
role and persistence checks. Observers receive the effective result and, when entered, the actual
terminal request.

Loop-owned event and batch identities use SHA-256 over length-prefixed session/conversation/turn identity and semantic phase suffix
under the `swarmie.agent.v2` domain.
The loop never places provider tool-call IDs directly into a TaskStorage identity. Envelopes
carry conversation, turn, task and step correlation; confirmed operation payloads additionally name the stable
operation ID. Cancellation request identity is independent of the current step. Append retries
keep the complete original payload, version, correlation and expected-head policy unchanged.

## Default checkpoint version two

A replacement handler registers task kind `swarmie.agent-turn`, definition version two, only if it
understands the persisted format and phase being resumed. The input is the public StartTurn DTO.
The checkpoint object has these exact fields:

| Field | Type and purpose |
| --- | --- |
| `version` | Required value 2 |
| `session_id`, `conversation_id`, `turn_id` | Persisted immutable causal identity, validated against task input |
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
`step-end` and `finish`. Version, identity and counter fields are required. The serializer emits explicit null for absent
receipt references and `ending`; a reader also accepts an omitted optional field as None, and
phases requiring a receipt reject its absence explicitly.
No content, arguments, model response or tool result is copied into this checkpoint. A compatible
replacement may support a narrower set of phases and explicitly refuse others. Different formats
must select a new definition version and implement the Durable Tasks migration contract, with
explicit acceptance or rejection. Native code replacement uses a fresh Host and preserved stores.

Version-one AgentLoop, Hook, tool-schema and policy descriptors are incompatible with the required
conversation fields. Version-two contracts have new fingerprints and generated bindings. There
is no default conversation and no format guessing. A version-one persisted task is explicitly
rejected through `TaskTransition::RejectMigration`; Durable Tasks preserves its input/checkpoint
and marks unsupported recovery instead of running effects. Mapping it to a conversation requires
independent evidence and an explicitly designed migration. The alternate implementation fixture supports version-two confirmed finalization checkpoints and
the exact `model` checkpoint with a correlated confirmed provider receipt for a final answer without
tool calls. It validates the admitted task, input, request and receipt before completing. It refuses
unknown effects and other phases. Default-loop recovery instead reenters the currently installed
model middleware and reuses the provider receipt, so transformations can finish without inference
resubmission. Native replacement does not promise identical behavior across arbitrary middleware
policies or unsupported checkpoint formats.

## Model qualification and external effects

Inference messages, content parts, finish reasons, usage, capabilities and sanitized provider
failures come from Model Contracts, without importing a provider SDK. Streaming uses its managed
generation Contract and bounded single-consumer event window. Incomplete tool snapshots are
observations only. The loop retains the generation Reference until physical local settlement,
acknowledges processed event prefixes and treats the final response as authoritative. The updated
neutral Contract adds `GenerationReference.recover()` and `ProviderErrorKind::Publication`. The loop
consults the retained live terminal authority whenever progress publication fails or remains behind,
including cancellation and cleanup. Confirmed recovered results are persisted before normal
checkpoint advancement. Recovery never repairs State by reissuing HTTP; the loop imports no network
provider implementation. Permanent failure of both callable live recovery and progress leaves settlement unconfirmed;
a timeout never fabricates completion. During Host retirement the Reference becomes Gone and
recovery is no longer callable. The loop yields its claim to Tembra disposal, whose Resource
closers join backend work. It writes no invented result; a started receipt without confirmation
remains uncertain and cannot authorize inference replay after restart.

Provider connection discovery must be unique. Unsupported effective capabilities fail before
submission; a capability marked model-dependent still requires remote acceptance. Unknown model
attempts park because remote billing or computation may already have occurred. External model
reconciliation references a correlated `model-provider-confirmed` event; tool reconciliation
references `tool-confirmed`. The operation must match the currently parked durable phase. The
loop does not offer an automatic unsafe replay permission.

The executable examples and agent acceptance suite use a synthetic provider implementing the
actual neutral Contracts. A concrete network adapter must additionally qualify credentials,
provider-specific continuation, usage omissions, streaming failure and remote cancellation limits.
Authorized ordered v2 opaque continuation extensions round-trip in their original message/content
positions, including signed tool continuations and original model tool-call IDs; the generic loop excludes hidden reasoning
prose. That exclusion is an explicit support constraint when an adapter requires other signed or
reasoning forms. It must be resolved through the owning Model Contract rather than a duplicate API.

Durable Tasks lifecycle notifications are independent observations. This loop does not subscribe
to them to decide execution progress or reconstruct conversation history. TaskStorage and Session
commits are reconciled by operation IDs and confirmed receipts, without assuming atomicity or
using best-effort notifications as a publication source. The current TaskStorage transactional
outbox remains independent from the loop's confirmed effect receipts. Generic terminal inspection can reconcile current
outcome; it cannot manufacture missing intermediate task transitions.

## Integration-owned task/session association

The loop consumes `TaskSessionBindings` from the independently owned task/session integration
capability. It does not implement a binding database or an outbox publisher. Binding identity
includes the actual source TaskStorage ID and task ID, destination SessionStore ID and session,
conversation and turn, root and optional parent, and routing version.

For a new root, `lookup` first retrieves any retained immutable admission intent. If absent,
`prepare` persists the complete initial TaskSpec and destination before `admit` may create the
Durable Task. Only the integration capability calls `DurableTasks.create`. Retries and concurrent
identical submissions reuse the winning initial checkpoint and duration origin. A conflicting
submission, explicit exclusion, or existing unbound task fails closed; a caller cannot invent
routing from its current conversation to repair missing evidence.

Every admitted or recovered phase requires `require_confirmed_binding` before phase execution.
That authority can confirm matching Pending intent even when the scheduler enters the handler
before the original admission call returns. Revision-one checks include initial checkpoint and
phase; later phases preserve immutable task input, identity, lineage and supported definition
version while allowing checkpoint progress. The consumer additionally compares the retained
Agent scope with the selected stores' authoritative IDs, then reads the durable task once and
requires it to equal the invocation. Historical `lookup` alone is not an execution grant.

Session receipt reads and Agent event appends, including post-effect recovery and cleanup,
require confirmed routing. They use the confirmation of their root: a turn-handler phase
invocation, a model-terminal invocation, a cleanup root or an AgentLoop control call confirms
once, before its first Session read or append. Providers and the code version are pinned for a
root, store identities are immutable, routing only moves from Pending to Active and sessions are
never deleted, so that confirmation holds for the whole root. Every Session page and append
receipt is still compared with the confirmed destination store, session and selector.
Control-surface Session reconciliation has the same requirement, so a replacement store
containing the same session name cannot receive a terminal event for another store. A phase
reads the current turn's receipts at most once and extends that view with its own appends.

Model and tool terminals still confirm again immediately before each effect, after asynchronous
preparation, middleware and policy awaits, together with a fresh durable task read and the
current cancellation, argument and permission checks. Optional hooks cannot bypass this
requirement. The model terminal also reads the durable task after its started receipt and
before activating the generation object, so a cancellation committed while its receipts are
appended ends the attempt without a generation or a provider receipt. Confirmation is not a
transaction spanning stores
or a promise of exactly-once external effects. Logical cancellation validates the actual source
and retained historical binding without depending on destination availability. It commits through
Durable Tasks first and performs no destination I/O on that control path. The integration publisher
owns delivery of retained source cancellation facts, without redirecting routing. Durable Tasks'
emergency `cancellation_unconfirmed` path retains its own semantics when outbox admission is
refused; this loop does not manufacture a missing transition fact. Continuing a turn or using
Session result evidence still requires full destination confirmation. Cancellation and physically
settled local work remain distinct from a durable confirmed result.

Child admission belongs to `prepare_child` and the existing scheduler's Wait transition. The
integration capability fixes inherited root, parent and destination before child creation;
optional conversation forks require explicit pre-creation intent. The default Agent Loop does
not introduce a child scheduler or an independent inheritance policy.

The agreed destination protocol uses identical complete immutable append batches with
`expected_head=None`, a stable mapping of source store/sequence/ordinal/version, and confirmation
of the destination durable receipt before source acknowledgement. Consumer identity belongs to a
managed Facet. Explicit acknowledged-prefix compaction, replay deduplication, supervision under
full outbox capacity and mapping compatibility belong to the integration-owned publisher.

## Executable examples

Both examples use a real Host, the existing scheduler, SQLite task and session stores, and the
integration capability's durable binding database. They obtain actual store IDs before installing
the bridge. Their scripted model and functional numeric tool are test fixtures. Temporary example
storage belongs to the example process.

```sh
just rust-run -p swarmie-agent-loop-fixture --example turn -- simple
just rust-run -p swarmie-agent-loop-fixture --example turn -- tool
```
