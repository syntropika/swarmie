# Task session integration

This compiled capability associates durable tasks with immutable session destinations and
publishes committed TaskStorage transitions into SessionStore. Tembra owns its References,
SQLite Resource and managed publisher activity. Select it explicitly alongside TaskStorage,
Durable Tasks and SessionStore; it is not enabled by the default profile.

## Configuration

```json
{
  "database": "bindings.sqlite",
  "task_store_id": "source-store-id",
  "session_store_id": "destination-store-id",
  "consumer_id": "session-history-v1",
  "auto_publish": true,
  "poll_interval_ms": 100
}
```

Obtain source identity from `TaskStorage.outbox_status()` and destination identity from
`SessionStore.publication_status().durable` or an authoritative session open receipt. The
binding database pins both store identities and the consumer ID. Keep the same database,
consumer ID and installed Facet identity across restarts. A different store pair requires
explicit configuration and a separate binding database; replacing a database with a same-named
session cannot adopt historical routes. Renaming, unlinking or replacing an open database or
its lock file is unsupported. Use a local filesystem supporting OS locks and SQLite WAL.

The public factory is `facet(&Configuration, Rc<dyn tembra::core::Driver>)`. It returns an
ordinary compiled Facet serving `TaskSessionBindings` and `TaskSessionPublisher`. Contracts
and portable DTOs belong to `swarmie-task-session-contracts`. The existing Host supplies the
Driver; there is no additional scheduler, event bus or hook infrastructure.

## Admission

1. Commit the session input and validate its exact session and conversation in the task owner.
2. Call `prepare` with a stable TaskSpec, the current source identity, routing version one and
   the full destination: store, session, conversation and turn. The complete pending intent
   commits before task creation. Identical retries return it; changed identities or payloads
   conflict without replacement. `lookup` can recover pending intent before a task exists.
3. Call `admit` with the store/task key. This capability owns root `DurableTasks.create` and
   reconciles its uncertain replies from the authoritative task. Retry the same key after a
   crash. Preparation alone does not automatically create work; admission must be requested.
4. At each recovered phase and immediately before each actual external effect, after any
   middleware, authorization or other awaits, require a confirmed binding. Compare its route
   with the task owner's persisted session/conversation/turn. The confirmation may activate a
   pending intent when the scheduler enters before `admit` returns. Missing, excluded,
   unavailable and mismatched associations refuse effects. Retain the task owner's independent
   cancellation, authorization and operation-idempotency checks.

Confirmation compares immutable task ID, kind, input and parent plus its supported definition
version. At revision one it also compares the original phase and checkpoint. Later checkpoints
may advance before admission returns. A task definition migration fails closed; this routing
version provides no automatic migration protocol. An active route is historical association,
not permission to execute a terminal or cancelled task.

Use `prepare_child` before returning the owning handler's `Wait` transition. It inherits the
parent's store/session/turn and root identity and freezes the child's own parent and stable task
ID. An optional conversation ID explicitly chooses a fork before submission. The task owner
then supplies the exact child TaskSpec to `Wait`; `admit` never creates a missing child as an
independent root. The same gate applies when child execution begins.

No transaction spans stores. Cancellation or an I/O/transport error can follow committed
intent, task creation, destination append or source ACK. Preserve the original key and request;
never infer absence, delete intent, recompute timestamps or reset execution limits after such
an error. An initial binding cannot be retroactively invented for an already existing task.
Restore lost routing intent from authoritative evidence or repair an explicitly unrelated task
with `exclude`. Exclusion is immutable and cannot replace a routed intent or authorize effects.

## Publication and retention

The managed Facet registers the configured consumer using its actual Tembra `caller_facet`.
It serializes automatic and explicit `publish` calls. A pass reads at most 256 facts in source
order, resolves the binding, confirms current stores and task identity, and maps one fact to
one Session batch. Stable domain-separated SHA-256 IDs derive from canonical source store,
commit sequence and ordinal. Mapping version one includes immutable structural source facts,
routing version and root/parent provenance; it adds no timestamp or current task snapshot.
Conversation, turn and task correlations remain attached to every destination event.

Every retry uses the identical batch with `expected_head=None`. The publisher validates the
returned store/session/batch identity and single-event receipt before acknowledging that fact.
A crash between append and ACK repeats the exact append and receives the original receipt.
Missing routes and failed receipts stop the contiguous ACK prefix. Only an explicit persisted
exclusion permits skipping an unrelated fact. Interleaved destinations retain source order
without implying atomic delivery of a source transaction across sessions.

Automatic activity resumes from the durable ACK after reopening. `auto_publish=false` allows
an embedding to drive the same bounded `publish` method explicitly through the managed Facet.
`diagnostics` reports authoritative capacity, positions, bounded retained lag and failure
categories. Its bounded source reads can advance the delivery watermark, never the ACK.
Lag is a lower bound when `lag_complete=false`. `capacity_exhausted` reports a reached retained
event/byte limit; remaining bytes may still be insufficient for the next fact. No diagnostic
proves physical disk headroom. Failure counters clear on restart; ACKs and routes do not.

ACK does not free retained capacity. Explicit `compact` delegates bounded deletion to
TaskStorage, which protects every registered consumer. Restore capacity by consuming and
explicitly compacting eligible history or increasing source limits after reopening. This
capability never removes consumers, deletes bindings, skips unknown facts or silently redirects
history. Retain routes and original TaskSpecs while retained facts or consumers need them.
Source outbox facts do not manufacture durable physical-settlement hook observations.

## Stable failures

| Reason | Meaning and recovery |
| --- | --- |
| `binding_missing` | No routing disposition; publication stops. Restore evidence or explicitly exclude unrelated work. |
| `binding_task_missing` | Intent exists but no matching task commit is confirmed. Retry root `admit`, or complete the owner's child submission. |
| `binding_conflict` | Immutable identity/request differs. Read the winning intent and validate it; never overwrite it. |
| `binding_excluded` | Explicit unrelated-task policy cannot authorize effects. |
| `binding_store_mismatch` | Current provider or database pins differ. Restore the selected store; never redirect historical facts. |
| `binding_task_mismatch` | Committed task identity/input/lineage/version differs from admission. Refuse effects and investigate. |
| `binding_fact_mismatch` | Source fact differs from its immutable route. Stop publication and inspect provenance. |
| `binding_admission_order` | Existing task has no prior intent. Restore authoritative intent instead of retroactive admission. |
| `binding_child_admission` | Missing child must be created by its owner's Wait transition. |
| `binding_version` | Unsupported mapping/routing version. Upgrade explicitly, without guessing a mapping. |
| `binding_session_receipt` | Append did not confirm the matching batch. Preserve the request and do not ACK. |
| `binding_io` | Storage outcome can be uncertain; retry/reconcile the same identity. |
| `task_storage_outbox_full` | Task creation can be refused while pending binding survives. Repair retention/capacity, then retry `admit`. |

Underlying Tembra and provider errors remain explicit. Closing the Host cancels managed
publication and releases SQLite ownership; it does not erase bindings, history or registrations.
