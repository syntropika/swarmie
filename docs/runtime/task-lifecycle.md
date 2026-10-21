# Durable task lifecycle hooks

Durable Tasks publishes small, typed observations through the existing Tembra series Hook
`swarmie.task-lifecycle`, version 1. Native Rust Facets and dynamic TypeScript Facets contribute
ordinary Tembra-managed handlers. TaskStorage remains authoritative: observers cannot change a
committed checkpoint, bypass ownership or CAS, undo cancellation, or turn a failed transaction
into a successful one. Existing task and storage Contracts retain their public signatures.

## Events

`TaskLifecycleEvent` is a discriminated union generated from the canonical Rust Contract in
`crates/task-contracts/src/lifecycle.rs`. Both variants carry `task_id`, `task_kind`, committed
reservation or record `revision`, `phase`, `attempt` and `definition_version`.

| Variant | Meaning | Additional fields |
| --- | --- | --- |
| `committed` | A TaskStorage receipt confirmed this record revision | `sequence`, `previous_status`, `current_status`, `origin`, `checkpoint_committed`, `cancellation_requested`, `outcome`, `blocking` |
| `phase_settled` | A managed invocation attempt and its owned physical work have settled | `invocation` (`run` or `migrate`), `outcome` (`returned`, `rejected`, `cancelled`) |

A reservation committed as `running` does not prove that the handler was invoked. Cancellation,
validation or admission refusal can prevent entry. A `phase_settled` rejection can also occur
before handler entry: the event proves settlement of the managed attempt, not execution of
user code. `returned` does not imply that the returned task decision validated or committed.
The settlement event uses the original admitted reservation revision, even if cancellation or
another permitted writer has since committed a later revision.

The committed event's origin is `creation`, `reservation`, `handler`, `recovery`, `cancellation`,
`retry` or `reconciliation`. Status and origin together avoid twelve overlapping pipelines:

| Observation | Selection |
| --- | --- |
| Creation | `origin = creation`, `previous_status = null` |
| Reserved phase | `origin = reservation`, `current_status = running` |
| Checkpoint | `checkpoint_committed = true` |
| Waiting or sleeping | `current_status = waiting` or `sleeping` |
| Crash recovery | `origin = recovery`; resulting status is pending or parked |
| Blocked or parked | `current_status = blocked` or `parked`; `blocking` is a small category |
| Explicit retry | `origin = retry` |
| Cancellation intent | `cancellation_requested = true` |
| Terminal outcome | `current_status = terminal`; `outcome` is a category, without result data |

`checkpoint_committed` marks accepted checkpoint, sleep, wait and migration decisions. Initial
creation includes the caller's initial checkpoint but does not set this flag.
`cancellation_requested` marks **newly persisted intent in this commit**, not the record's
current cancellation flag. It can coexist with a terminal transition in one event. Completing
records may carry a held outcome; only `current_status = terminal` confirms terminality.
A batched transaction emits at most one committed event per returned record, with the same
commit sequence. Reconciliation without a new write emits nothing.
These observations cover scheduler-managed commit receipts. Direct TaskStorage writes and
startup scans do not synthesize notifications for historical transitions.

Inputs, checkpoints, child lists, output bodies and diagnostic strings are excluded. Identity
and phase strings are bounded to 128 characters; revision and sequence fit exact portable JSON
integers. The complete event remains below 2 KiB even with maximal metadata. Read TaskStorage
explicitly for full state; a later read can already contain a newer revision.

## Contributing from Rust

Declare a local application Contract whose `observe` method is exactly
`task_lifecycle()?.descriptor().contribution_member()`. Register its method using the returned
`RustHookDefinition` and wrap it with Tembra's `hook_handler`. The callback receives
`HookArgs<TaskLifecycleEvent, ()>` and a managed `Context`:

```rust
let definition = swarmie_task_contracts::task_lifecycle()?;
// Inside the owning ordinary Tembra object declaration:
definition.register(declaration, "audit", -10, "observe")?;
```

The real native example in `tests/plugins/fixtures/durable-tasks/src/hooks.rs` shows the full
Facet, local Contract and managed implementation. Contributions use unique keys. Tembra orders
handlers by ascending signed priority, then Registry insertion order for ties. There is no
separate observer registry, callback execution engine or scheduler.

## Contributing from TypeScript

Run the existing root Contract-generation workflow before compiling plugins. Import the typed
Hook and event from the Durable Tasks authoring helper and use Tembra's ordinary declarations:

```typescript
import { Type, defineFacet, hookHandler, local, registerHook } from "@tembra/sdk";
import { TaskLifecycle } from "./plugins/durable-tasks/typescript";

const Audit = local("application.task-audit", {
  version: 1,
  input: Type.Object({}),
  members: { observe: TaskLifecycle.method },
});

export default defineFacet({
  name: "application.task-audit",
  uses: [],
  setup(_scope, declaration) {
    declaration.serve(Audit, (_input, self) => {
      registerHook(self, TaskLifecycle, "audit", "observe", { priority: -10 });
      return {
        state: {},
        methods: {
          observe: hookHandler(TaskLifecycle, (_context, { input: event }) => {
            if (event.type === "committed" && event.current_status === "terminal") {
              console.log(event.task.task_id, event.task.revision, event.outcome);
            }
          }),
        },
      };
    }, { lifetime: "host" });
  },
});
```

Import paths depend on the plugin's location. The event DTO derives from generated Rust schema;
the TypeScript Hook descriptor has the same tested fingerprint. Swarmie's dynamic plugin
Executor installs the exact typed language adapter before plugin activation. An embedding that
bypasses the Plugin Manager must register a matching typed Tembra Executor adapter itself.
The runnable portable example is `tests/plugins/fixtures/durable-tasks/observer.ts`.

## Delivery and failure policy

Emission never awaits a plugin inside a transaction, reconciliation lock or task cancellation
path. A capability-local queue feeds one ordinary Tembra-managed activity; Tembra owns Registry
plans, priority, invocation, cancellation and physical settlement. Each run uses a fresh current
generation root. Contributors can call task APIs without holding the scheduler's lock.

The queue holds at most 256 events. One physical Hook run and up to 64 contributions are admitted.
Overflow drops the new notification and increments `dropped`; it does not block task execution.
A Registry emptiness probe skips queue allocation when no contribution exists. Registration
between this probe and invocation is a best-effort race, not a delivery subscription boundary.
Synchronous delivery yields through the Host Driver after each batch of sixteen events.

Series is fail-fast: a failing or cancelled handler prevents later handlers in that event's
plan from running. Its error is retained in diagnostics and subsequent events remain eligible.
Handlers that need local error recovery can handle their own failure explicitly. No handler
result can modify the persisted event or task transaction.

After one second, the observer Context is cancelled. The dispatcher still waits for actual
Tembra settlement; a deadline never claims physical termination. Non-cooperative owned work
can hold the observer slot and delay Host close indefinitely. The scheduler remains independent
and the queue stays bounded. Cancelling the original task-API caller does not cancel an already
admitted independent observation. Task cancellation and observation cancellation have distinct
ownership.

Withdrawal and compatible dynamic replacement use Tembra's contribution lifecycle and current
Registry generation. A snapshotted admitted invocation can finish on its original generation;
subsequent plans select the current contribution. Removing a contribution does not replay events
to a replacement. Native declaration replacement retains the existing fresh-Host requirement.

During Host shutdown, admission stops, queued notifications are abandoned and counted, and any
active run is cancelled and physically joined. Graceful reservation restoration in the Resource
closer deliberately emits no notification: observation admission is already closing. A process
crash bypasses that cleanup; its next confirmed recovery transition can be observed.

Use the separate read-only `TaskLifecycle` Contract's `diagnostics()` method to inspect process-local
`skipped`, `enqueued`, `completed`, `failed`, `dropped`, `abandoned`, `timed_out`, `queued`, `active`
and `last_error`. Completed means the series run succeeded, not external durable delivery.
Failed includes Registry/admission/run failures and live commit-sequence projection failures;
`last_error` is bounded to a 512-byte UTF-8 prefix. Counters saturate at the portable integer limit
and reset with a fresh Facet. Diagnostics are operational observations, not a task history.

## Crash and deduplication guarantees

A successful TaskStorage commit and lifecycle notification are separate facts. A crash can occur
after the commit but before admission, between admission and invocation, or after some observer
side effects. Hooks provide no durable delivery, automatic replay or exactly-once execution.
CAS conflicts, validation failures and unknown I/O outcomes emit no invented committed event.
Reading state after an ambiguous I/O failure cannot reconstruct which hook should have run.

Within one task store, `(task_id, revision)` identifies a committed record transition.
`(task_id, reservation_revision, attempt, phase_settled)` identifies a physical-attempt observation.
Consumers spanning stores must add their own configured source namespace. Retain the variant
when deduplicating because a reservation event and settlement can share a revision. Commit
sequence groups records from one transaction; cross-caller delivery order is not a transactional
ordering guarantee. Normal scheduler reconciliation never duplicates terminal notifications.

## Future capabilities and durable publication

Each capability owns its hook semantics. Task definitions can declare their own Tembra Hook
vocabularies and filter technical events by task kind. Future agent or tool capabilities can
choose waterfall, bail or middleware policies that transform context, arguments or results.
Those policies are separate from these immutable post-commit observations.

A future Session Event Log that must retain every selected task transition needs a transactional
outbox in TaskStorage. The task record change and a bounded structural event must commit in the
same storage transaction. A proposed event identity is `(store_id, sequence, ordinal)` plus task
ID and revision; delivery to the destination is at least once with idempotent ingestion and a
durable acknowledgement cursor per consumer. Snapshot reconciliation restores current state,
not overwritten intermediate transitions or checkpoint history.

That extension needs a versioned public storage Contract, provider conformance tests, resumable
tail queries, explicit consumer registration and lag policy, retention watermarks and byte limits.
Required consumers can hold a retention watermark until acknowledged; optional consumers may
lose retained history only with an explicit gap and snapshot-resynchronization protocol. A full
outbox must reject the whole task transaction or use an explicitly agreed retention policy,
never silently discard a required audit event. Physical invocation observations cannot pretend
to be atomically persisted with a separate task transaction. Do not automatically make every
Hook event durable. The outbox, Session Event Log and Agent Loop remain separate future work.
