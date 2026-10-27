# Agent Loop

Agent Loop runs model turns and tool calls. The same engine supports direct calls,
durable tasks, and stored conversation history. It does not require PluginManager.

## Choose an adapter

| Use | Entry point | Required feature |
| --- | --- | --- |
| Caller-owned context | `Agent::execute` and `Agent::resume` | None |
| Task checkpoints with inline context | `durable::execute` | `durable-tasks` |
| Stored history | `conversation::History` | `conversations` |
| Atomic history and task checkpoints | `conversation::durable::execute` | Both |

These are library choices. Swarmie agent executions use the durable conversation
adapter. Independent library consumers can use ordinary execution without storage.

## Context and progress

`AgentContext` contains instructions and model-visible messages. `Request` adds the
allowed tools and turn limit. `State` contains the confirmed request, phase, usage,
and pending operation. It contains no live provider or storage handles.

```rust
use std::{num::NonZeroU32, rc::Rc};
use swarmie_agent_loop::{
    Agent, AgentContext, ExecutionResult, Message, Model, Request, Tools,
};

async fn reply(
    context: &tembra::Context,
    model: Rc<dyn Model>,
    tools: Rc<dyn Tools>,
) -> ExecutionResult {
    let request = Request {
        context: AgentContext {
            instructions: "Answer briefly.".into(),
            messages: vec![Message::User { content: "Hello".into() }],
        },
        tools: vec![],
        max_turns: NonZeroU32::new(4).unwrap(),
    };
    Agent::new(model, tools).execute(context, request).await
}
```

Direct callers own the physical future. Use the supplied Orb for a Tembra-managed
invocation when a dropped waiter must not abandon work. Preserve the inner
`ExecutionResult` when calling through Tembra: it carries confirmed progress with
the original error. Invalid initial state has no progress. If `failure.state` is
present, pass `*state` to `Agent::resume` to continue from confirmed progress. Its
presence does not permit replay: pending operations still require provider recovery.

## Confirmation and recovery

The engine confirms the effective model request before dispatch. It then confirms
the response before it can execute tools. It also confirms the final tool arguments
and provider identity before each tool call, then confirms the result before advancing.

A resumed pending operation first calls provider recovery:

- `Completed` uses the known result.
- `SafeToRetry` permits another dispatch. A tool retry also checks current permission
  and the confirmed provider identity.
- `Unknown` leaves the operation pending and requires reconciliation.

Cancellation cannot undo an external effect. Providers own effect recognition and
recovery; the engine does not infer success from a lost response.

`durable::execute_with` can stage caller-owned writes with each checkpoint. Use only
the supplied write context and await every staged call. The storage owner must
support all writes in that commit. `State::messages()` is cumulative for the whole
execution. Appending that full slice at each confirmation duplicates history. Track
entry identities or the append position in the same commit. External effects stay
outside the transaction.

To compose the inline durable adapter, install `durable::orb(factory)` and include
`durable::definition()` in the scheduler's `Definitions`. Its task input is a
serialized `swarmie_agent_loop::Request`. The factory returns the configured `Agent`.
For stored history, use `conversation::durable::orb(factory)` and its matching
`definition()`; that factory returns `(Agent, History)` and its task input is a
serialized `conversation::Request`.

## Hooks

Tembra owns contribution order, delivery, and object lifecycle. The supplied Orbs
declare and bind these typed hooks. A custom Orb must declare `Hooks` and call
`Agent::bind_hooks`. An unbound `Agent` does not dispatch contributions.

| Hook | Purpose |
| --- | --- |
| `BeforeTurnAgent` | Prepare the first model turn |
| `BeforeTurn` | Prepare each new model turn |
| `PrepareContext` | Project the request context before intent confirmation |
| `BeforeToolCall` | Change arguments or block a newly prepared call |
| `AfterToolCall` | Transform a tool result before confirmation |
| `AfterTurn` | Observe a confirmed turn after its tools finish |
| `AfterTurnAgent` | Observe completion or interruption of one physical attempt |

Request hooks cannot change the turn or widen allowed tools. Tool hooks cannot
change call identity or name. Prepared projections remain separate from canonical
history. Request and argument transformations can run again if execution stops
before confirmation. Result transformations can also run again during recovery.
Keep all transformations free of external effects.

`BeforeToolCall` does not run again for a pending call. The provider must enforce
authorization during recovery and retry. Contributions target a specific declaring
Orb; ordinary and durable Orbs do not share contributions automatically.

An observer error does not replace the primary result. `observation_failure()` holds
the most recent observation error for a bound Agent and its clones. It is a volatile
diagnostic, not a per-invocation log or proof of task completion. Bind one drive to
one entered Tembra invocation; terminal observation is limited to that source.

Observers can be skipped or repeated after a crash. `ExecutionStatus::Completed`
means confirmed loop progress, not a confirmed `TaskStatus::Completed` result. The
scheduler can confirm the task result later. Terminal observation retains the
original principal and Tembra plan, but opens no storage write scope and does not
revive a closed scope. Cancellation is cooperative; a hook that does not settle can
delay host close.

## Streaming

Use `Agent::with_progress` with a bound `swarmie_providers::Progress` to request provider
streaming. Previews are transient. They do not enter history, checkpoints, or tool
dispatch. Only the settled response enters the normal confirmation path.

## Stored conversations

`conversation::History` reads `Record::Message` values from a fixed history prefix.
Its checkpoint stores entry references instead of another transcript. The restored
`State` still materializes model-visible messages in memory. Prepared model requests
are separate evidence records. Tool arguments use a `PreparedTool` record only when
they differ from the original call or include a routed provider identity. Otherwise,
the checkpoint refers to the original assistant call.

With both features, the conversation adapter commits new history and task progress
through one compatible storage owner. A stale head causes a conflict. The adapter
does not merge competing appends or replace the new tail. Both checkpoint-only and joint
confirmations report `RecoveryRequired` for storage `Conflict` or `Unavailable`.
The last confirmed checkpoint and history remain the recovery point. See
[Conversations](conversations.md) for history selection and write receipts.

## Scope

Checkpoint migration is not implemented. Resume requires a checkpoint that the
current adapter can decode and validate. This guide covers the native Rust loop
and its adapters. It does not qualify JavaScript plugin loading or mixed-language
hook execution.

## Source map

- `engine.rs`: phase transitions and confirmation order.
- `engine/operations.rs`: hook preparation and provider recovery/dispatch.
- `durable.rs`: task checkpoint adapter.
- `conversation/driver.rs`: managed history reference and execution entry points.
- `conversation/restore.rs`: history loading and checkpoint validation.
- `conversation/writer.rs`: proposed records and storage confirmation.
