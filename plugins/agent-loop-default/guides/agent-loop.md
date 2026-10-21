# Agent turns and lifecycle extensions

The `swarmie.agent-loop` version-two Contract starts, inspects, cancels and continues bounded
agent turns. `swarmie-agent-loop-default` supplies one compiled implementation. Other compatible
implementations serve the same Contract and register the same task kind and checkpoint version.
Tembra selects providers and owns actual execution. Durable Tasks is the only scheduler.
Native implementation replacement requires a fresh Host; dynamic hook contributors use Tembra
JavaScript reload.

## Authorities and submission

A caller first commits a user `swarmie.message` through SessionStore. `StartTurn` identifies that
session, required conversation, input event, turn, model connection/model and independent step, tool, duration and output
token limits. Repeating the exact submission reconciles the existing task; changing a request
under the same session/conversation/turn identity is refused. The same turn ID can be used in
different conversations. Admission verifies that the committed input is a user message in the
selected session and conversation, using bounded-memory filtered pagination. Task, operation and
event IDs hash length-prefixed session/conversation/turn/phase identity under a version-two domain. Model request identity is correlation, never a provider idempotency key.

SessionStore owns conversational history and confirmed external results. TaskStorage owns task
status, cancellation and durable transitions. Neither State, hooks nor a successful method return
substitutes for a storage commit. A turn checkpoint contains counters, deadline origin, source
position, explicit version, persisted session/conversation/turn identity and event references. It does not copy messages, model bodies or tool output.

The default loop runs resumable phases for turn start, context/request preparation, generation,
response processing, tool preparation, tool execution, result processing, step end and finalization.
The current implementation uses one durable turn task: each phase is a scheduler checkpoint,
not a second executor or an independent task per token. A future independently managed long-running
tool can own child tasks through the existing Durable Tasks Contract. No child-task scheduler is
introduced by this capability.

## Native lifecycle hooks

Each hook has its own version-two Tembra descriptor, portable typed schema and lifecycle-owned
Registry. Lower numeric priorities execute first, using Tembra's deterministic ordering. Every
run snapshots the actual admitted contribution plan. Empty observers succeed; empty waterfalls
return the original input. A control-hook failure stops its phase. Series still fails fast within one observation pipeline,
but observation errors do not fail the turn or invalidate a confirmed operation. Delivery is
best-effort and can be absent or repeat on recovery; observers must make their own effects safe
to repeat.

| Hook | Strategy | Responsibility |
| --- | --- | --- |
| `on_turn_start` | series | Observe a started turn |
| `select_context` | waterfall | Select bounded immutable source ranges before materializing history |
| `prepare_context` | waterfall | Transform the model-message projection |
| `before_model_call` | waterfall | Transform request options and content before durable intent |
| `wrap_model_call` | middleware | Wrap the real managed model terminal through Tembra Next |
| `on_model_event` | series | Observe ephemeral sequenced stream events |
| `after_model_call` | series | Observe a confirmed complete model outcome |
| `before_tool_call` | waterfall | Transform final tool arguments |
| `authorize_tool_call` | bail | Optionally veto a tool invocation |
| `after_tool_call` | series | Observe a confirmed tool outcome |
| `on_step_end` | series | Observe a fully processed step |
| `on_turn_end` | series | Observe a turn decision; actual task commitment remains separate |

Context and request hooks cannot change causal identity. The loop checks those fields after
transformation and caps model output admission to the remaining turn budget. Model middleware
executes an ordinary managed terminal; it may explicitly supply a replacement result. The terminal
independently validates the authoritative task, model identity, limits and capabilities. Tembra
Next enforces continuation ownership and explicit error recovery. A middleware failure never
undoes a completed external request.

Tool middleware is deliberately absent from this implementation. Argument transformation and an optional
veto cover the initial use cases without permitting a wrapper to bypass execution policy.
`ToolPolicy.authorize` is mandatory immediately before real tool admission. Tool providers must
also enforce their own required authority. A missing policy cannot authorize a tool. Optional
hook participation conveys no execution grant and does not require the loop to import contributors.

## Best-effort observation lifecycle

The six series hooks are observations: turn start, model stream, confirmed model result, confirmed
tool result, step end and turn end. The loop admits notifications after their corresponding
Session confirmation; stream events remain ephemeral. One Tembra-owned activity runs the real
HookManager in FIFO admission order across turns. Tembra preserves priorities and fail-fast
behavior inside each pipeline and owns handler invocation and physical settlement. The observer
Context is independent of the turn caller; handlers can finish after the task becomes terminal.

Admission is limited to 64 queued notifications and 4 MiB of encoded queued payload, with at most
1 MiB per notification and one active pipeline. At most 64 contributions are admitted per Hook
run. Absent handlers skip queuing. Queue/byte pressure drops new observations, with diagnostics;
turns and streaming acknowledgements continue. Each pipeline has a one-second cooperative deadline.
Cancellation still joins physical work before starting the next observation. A noncooperative
handler can delay later observations and Host closure, but does not hold a turn phase or model
Reference. Shutdown abandons pending notifications instead of promising delivery.

`AgentObservations.diagnostics()` exposes bounded volatile counters, queue occupancy, active state
and a bounded reason identifier, without payloads or handler error messages. These diagnostics do
not depend on reactive State publication. Observation failure never creates a phase error or rolls
back a result. A repeated durable phase can admit a duplicate; a crash can lose a queued event.
Normal repeated terminal inspection does not resend the end observation after its decision receipt
exists. Consumers needing durable audit delivery must use the integration-owned outbox publisher.

## Model and tool boundaries

The loop imports only neutral Model Contracts. A model-provider Registry entry resolves a unique
connection to independently named provider and generation Contracts. A generation Reference is
retained through `start`, sequenced event processing, batched acknowledgements and physical local
settlement. When State is stale, unreadable or a start/acknowledgement publication fails,
`recover()` reads the same live generation's retained result. Cleanup uses a fresh managed Facet
root and also recovers after cancellation, confirms local physical settlement and attempts to
persist that result before releasing the Reference. It never sends inference again to repair
reactive publication. Losing the object or Host loses volatile recovery; confirmed Session receipts
are the crash-recovery authority. Host retirement makes generation methods inadmissible before
resource closure. Once the Reference is Gone, the loop yields disposal to Tembra, whose Resource
closers own local work joining; it does not wait forever on inaccessible recovery calls or invent
a terminal result. A missing confirmed receipt remains uncertain on reopening. The final response
is authoritative; partial tool JSON never causes execution.
Reasoning deltas are excluded from generic stream observers, and hidden reasoning prose is not
persisted. Authorized opaque continuation extensions remain intact. Providers requiring additional
continuation forms need explicit representation and qualification before support is claimed.

Usage comes from the provider. Missing output usage conservatively consumes the admitted output
cap; this may stop another step sooner. The loop invents no prices or costs. Wall time includes
waiting and recovery. Deadline cancellation signals active managed work and awaits physical
settlement; arbitrary noncooperative external work cannot be forcibly rolled back. Finalization
has a bounded cooperative grace period. An aborted local HTTP stream does not prove that remote
computation or billing stopped.

Tools contribute named managed operations through the actual Tembra Registry, matching the
canonical `ToolHandler.execute` method schema. Metadata carries name, argument schema and replay
policy. The final transformed arguments are schema-checked again before execution. Tool results
retain both model-issued call ID and loop-owned operation ID. An ordinary confirmed failure is
`ToolResult { is_error: true, ... }`; a transport or cancellation error without a confirmed result
may remain uncertain and must not be fabricated as a successful outcome.

## Crash recovery and separate commits

For each operation, request/argument preparation is persisted before the task advances to its
effect phase. The effect phase first checks for a confirmed Session receipt. If it exists, the
phase reuses it without invoking the provider. Otherwise, it appends a deterministic started
receipt before admitting the external operation. Stable one-event batches use `expected_head:
None` and no changing timestamp. An uncertain append is retried with the identical complete
request, including its original batch ID and payload.

A process can stop after the external effect and before the confirmed receipt. A started receipt
without a result therefore means uncertainty. Model requests are never automatically resent in
this condition. Tools replay only if both the persisted and currently registered policies are
`safe`, meaning read-only behavior or durable deduplication of the stable operation ID. An `unsafe`
operation parks the task and retains all evidence. This protocol does not promise exactly-once
external effects. A receipt may conservatively indicate uncertainty even if the effect never began.

`continue_turn` can associate a committed, correlated result event with an uncertain operation
through `KnownResult`. It verifies the event's turn, task, kind, version and operation identity.
It does not offer a blanket permission to repeat unsafe work. Retrying without adequate evidence
parks again. A confirmed result can be ahead of the TaskStorage checkpoint; recovery discovers
that result and resumes processing. Checkpoints cannot reconstruct a missing external result.

`turn-ended` records the loop decision before the scheduler commits it. Cancellation can win
that race. `task-settled`, reconciled when the control surface inspects a terminal task, records
the authoritative task outcome separately. The Agent cancellation control verifies the source
and historical binding, then cancels through Durable Tasks without destination I/O. Its source
retained transition facts are delivered by the integration publisher, subject to Durable Tasks'
explicit emergency cancellation semantics when outbox admission is refused. Cancellation, loop decision and physical or
durable settlement are distinct facts. If cancellation bypasses the normal finalization phase,
terminal inspection reconciles its Session event and admits an end observation. Hook notifications
are not the source of execution truth. Durable Tasks has its own transactional outbox; the loop
does not use ephemeral task hooks to reconstruct durable transitions.

## Current admission and verification

Before reading history, `select_context` receives the admitted input position and a frozen Session
head, plus explicit sequence intervals and shared budgets. The default selects the conversation
prefix through the input and this turn's later rows. Contributions can select up to 16 ordered,
nonoverlapping prefix, suffix or summary intervals without materializing the full conversation.
Every query requires the persisted conversation identity; turn-only intervals additionally bind
the persisted turn. At least one conversation interval must include the admitted input. A plugin
can consult SessionStore through its Tembra Context or inject an independently retrieved summary
through `prepare_context`; the loop neither changes source events nor implements memory storage.

The combined admission limit is 4 MiB and 4096 source events across all intervals. Hooks may reduce
these budgets. Overlapping/out-of-range intervals, a changed identity/head/input, omitted input,
excess rows or bytes fail explicitly. No silent truncation or full-session fallback exists.
Operational receipt lookup separately filters the current conversation and turn, so a large
conversation does not prevent reaching context selection. User-input lookup uses bounded page
memory and has cost proportional to the filtered prefix. Control-hook transformations and model
middleware remain subject to final-boundary identity, capability, token and duration checks.
Unknown roles and unsupported transcript kinds are not interpreted as model instructions.

Canonical Rust descriptors generate TypeScript through the root Tembra tooling. The optional
`javascript` feature exports `register_agent_hooks` to register the real executor's portable
codecs through its compiled Core Plugin `with_language_adapter` declaration. The generic Runtime
initializes all declared adapters before loading dynamic contributors or cached recovery; it
contains no Agent-specific hook registration in bootstrap. Compiled distribution metadata comes from the same
workspace tooling. Synthetic models, tools, policies and context plugins remain acceptance fixtures.

Run the root recipes with pinned tools:

```sh
just tembra-plugins
just rust-test -p swarmie-agent-contracts -p swarmie-agent-loop-fixture
just rust-fmt-check
just rust-clippy
just rust-doc
```

See [integration contracts](agent-loop-integration.md) for event conventions, the default
checkpoint format, provider qualification and executable examples.
