# Agent turns and lifecycle extensions

The `swarmie.agent-loop` version-one Contract starts, inspects, cancels and continues bounded
agent turns. `swarmie-agent-loop-default` supplies one compiled implementation. Other compatible
implementations serve the same Contract and register the same task kind and checkpoint version.
Tembra selects providers and owns actual execution. Durable Tasks is the only scheduler.
Native implementation replacement requires a fresh Host; dynamic hook contributors use Tembra
JavaScript reload.

## Authorities and submission

A caller first commits a user `swarmie.message` through SessionStore. `StartTurn` identifies that
session, input event, turn, model connection/model and independent step, tool, duration and output
token limits. Repeating the exact submission reconciles the existing task; changing a request
under the same turn ID is refused. Task IDs and operation/event IDs derive deterministically from
the turn and phase identity. Model request identity is correlation, never a provider idempotency key.

SessionStore owns conversational history and confirmed external results. TaskStorage owns task
status, cancellation and durable transitions. Neither State, hooks nor a successful method return
substitutes for a storage commit. A turn checkpoint contains counters, deadline origin, source
position and event references. It does not copy messages, model bodies or tool output.

The default loop runs resumable phases for turn start, context/request preparation, generation,
response processing, tool preparation, tool execution, result processing, step end and finalization.
The current implementation uses one durable turn task: each phase is a scheduler checkpoint,
not a second executor or an independent task per token. A future independently managed long-running
tool can own child tasks through the existing Durable Tasks Contract. No child-task scheduler is
introduced by this capability.

## Native lifecycle hooks

Each hook has its own version-one Tembra descriptor, portable typed schema and lifecycle-owned
Registry. Lower numeric priorities execute first, using Tembra's deterministic ordering. Every
run snapshots the actual admitted contribution plan. Empty observers succeed; empty waterfalls
return the original input. A handler failure stops its pipeline. Delivery can repeat on recovery;
observers must make their own external effects safe to repeat.

| Hook | Strategy | Responsibility |
| --- | --- | --- |
| `on_turn_start` | series | Observe a started turn |
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

Tool middleware is deliberately absent from version one. Argument transformation and an optional
veto cover the initial use cases without permitting a wrapper to bypass execution policy.
`ToolPolicy.authorize` is mandatory immediately before real tool admission. Tool providers must
also enforce their own required authority. A missing policy cannot authorize a tool. Optional
hook participation conveys no execution grant and does not require the loop to import contributors.

## Model and tool boundaries

The loop imports only neutral Model Contracts. A model-provider Registry entry resolves a unique
connection to independently named provider and generation Contracts. A generation Reference is
retained through `start`, sequenced event processing, batched acknowledgements and physical local
settlement. The final response is authoritative; partial tool JSON never causes execution.
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
the authoritative task outcome separately. Cancellation request, loop decision and physical or
durable settlement are distinct facts. If cancellation bypasses the normal finalization phase,
terminal inspection delivers the end observer and reconciles its Session event. Hook notifications
are not the source of execution truth and cannot replace a future transactional task outbox.

## Current admission and verification

The default context reads an immutable paginated Session prefix, including the current turn's
later messages, with a 4 MiB / 4096 event admission ceiling. A context plugin can inject memory or
compress the admitted projection, without rewriting source history. Longer sessions require a
qualified selective-context strategy; version one does not silently truncate history. Unknown
speaker roles and unsupported transcript kinds are not interpreted as model instructions.

Canonical Rust descriptors generate TypeScript through the root Tembra tooling. The optional
`javascript` feature exports `register_agent_hooks` to register the real executor's portable
codecs before loading dynamic contributors. Compiled distribution metadata comes from the same
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
