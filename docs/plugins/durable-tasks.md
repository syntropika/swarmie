# Durable Tasks

Durable Tasks stores work so another attempt can continue after interruption.
It owns assignments, leases and confirmed progress. A task handler owns its domain
logic and the recovery of external effects.

## Define a task

A task is a normal Tembra service with `TaskMethod::Execute`. The Orb's API version must
match `TaskDefinition::version`. Each attempt receives a fresh service instance,
keyed by task ID and attempt ID.

```rust
use serde_json::json;
use swarmie_durable_tasks::{Execution, TaskMethod, TaskOutcome};
use tembra::{Orb, Service};

let worker = Orb::new("example.worker", 1).service(
    Service::new("example.worker", |_| Ok(())).method(
        TaskMethod::Execute.as_str(),
        |_, context, execution: Execution| async move {
            let input = execution.input();
            execution.commit(&context, json!({ "phase": "checked" })).await?;
            Ok(TaskOutcome::Completed(input))
        },
    ),
);
```

Read `execution.checkpoint()` when an attempt starts. Only confirmed progress is
returned. Transient resources do not survive a wait or a process restart.
`Execution` is a native capability; it cannot be constructed from portable input.

## Compose and start

The embedding supplies the host, a lazy `StorageFactory`, `Definitions` and
`Options`. Pass them to `orb(host, storage, definitions, options)`. Keep the service
reference alive and the local executor running until shutdown finishes.

`Definitions::new` accepts typed task definitions. The optional `plugin-manager`
feature can derive them from `PluginIndex` through `Definitions::from_plugins`.
The index contains metadata. Tembra resolves and invokes the actual service.

## Methods and authority

| Method | Caller and result |
| --- | --- |
| `Create` | Stores a task under the invoking principal. |
| `Signal` | The task owner or host can supply a signal. |
| `Get` | Returns a full record visible to its owner or host. |
| `List` | Returns caller-visible metadata, capped at 1,000 rows per page. |
| `Resume` | The host enables admission and requests recovery. |
| `Wake` | The host requests reconciliation and local recovery. |
| `Inspect` | The host reads scheduler status and diagnostics. |

A principal cannot inspect another principal's tasks. The trusted host can inspect
all tasks; this grants no authority over their external resources.

The scheduler starts paused. Call `Method::Resume` through the singleton service
with `HOST_PRINCIPAL` and `INSTANCE_KEY`. Resume returns promptly; it does not own
the scheduler lifetime. `Method::Create` accepts a one-element `(TaskSpec,)` tuple
and stores the caller's principal with the task.

Options require a local spawner, a nonzero lease duration and an application timer.
`max_executions` defaults to `0`, which means no configured local limit. `Fifo`
orders work by creation sequence. A custom `SchedulerStrategy` must return each
candidate exactly once within the offered metadata page.

## Save and finish

- `commit(context, checkpoint)` confirms progress before the handler continues.
- `commit_with(context, prepare)` confirms staged domain writes and the returned
  checkpoint in one compatible storage owner transaction. Await all participating
  calls and use the context supplied to the callback.
- `complete(context, result)` confirms a result before physical cleanup ends.
  Its assignment and local slot remain held until that cleanup settles.

Only a successful commit confirms the new checkpoint. A refused write leaves the
last confirmed value intact. A commit with an unknown outcome freezes the shared
storage owner. It cannot be retried as if nothing happened.

The callback used by `commit_with` prepares logical writes, not a live SQL
transaction. A successful nested call only acknowledges staging. External effects,
detached calls and writes from another owner are outside this atomic commit.
See [Storage](../storage.md).

## Wait and receive

Return `TaskOutcome::Waiting { checkpoint, condition }` to save progress and
release execution capacity. `WaitFor::Delay` uses the storage authority clock.
`WaitFor::Signal` waits for a typed `SignalKey`.

`Method::Signal` takes `(TaskId, SignalKey, Value)`. The task owner or host may send
it. A signal can arrive before the wait. Its receipt preserves its value and
consumption state: equal duplicates are accepted; conflicting values are refused.
A consumed key cannot be used for a new wait. Receipts have no expiry policy in
this implementation.

## Recover and stop

The scheduler requests renewal every third of the configured lease, with a
one-millisecond minimum. Size the lease for storage latency and keep the timer
operational. Each progress commit and renewal checks the attempt token and
authority clock. An expired attempt cannot renew, save progress or overwrite a
successor. Releasing a claim cannot release a successor.

Storage and definition subscriptions request replanning. They are local hints,
not commit authority. A host adapter must wake the scheduler after changes made
elsewhere. Metadata reads and completion groups are bounded so control and timers
can run between groups.

Missing definitions remain blocked and do not prevent healthy work from running.
Interrupted executions can remain held until an explicit `Wake`, `Resume` or
relevant definition change requests recovery. This hold is local to the scheduler.
After reopening, ready tasks can be admitted again. Handlers and providers must reconcile pending effects before repeating them.

Resource cleanup from an earlier attempt can overlap a later attempt. Finish
assignment-dependent work in the awaited handler or managed writes before returning.
The engine cannot discover detached futures. Await every call that belongs to the
attempt.

Close stops admission, requests cancellation and waits for physical handlers and
managed writes. Cancellation is cooperative; an uncooperative handler delays close.
Keep the executor and timer alive during this wait. After abrupt process exit,
assignment expiry permits another attempt.

Cancellation cannot undo an external operation. Providers must recognize or query prior effects
before repeating them; an unknown result grants no replay authority.

## Implementation map

- `scheduler/`: policy, scheduling loop, candidate selection and attempt lifetime.
- `execution.rs`: handler capability and managed checkpoint writes.
- `orb.rs`: service methods and the managed writer invocation.
- `model.rs`: task identities, outcomes and persisted progress.
- `authority.rs`: storage outcomes and shared recovery gate.
- `storage.rs`: backend-neutral adapter interface and change values.
- `transitions.rs`: atomic task and signal state transitions.
- `definitions.rs`: validated task contribution metadata.
