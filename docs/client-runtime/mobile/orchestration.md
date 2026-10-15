# Routines and durable work

Routines are currently set aside from the active mobile product scope. The
sections below record the existing local runtime implementation and its limitations.
When routines return to the mobile product, Swarmie Cloud must own scheduling. These local
runtime mechanisms do not establish the hosted scheduler contract or its implementation.

## Current runtime map

`crates/core` owns session state, prompt dispatch, queueing, cancellation and harness-independent
submission receipts. `crates/storage` retains canonical ordered session events and submission records
in SQLite. `crates/adapters` supplies harness-specific dispatch, configuration, status and cancellation
capabilities. `crates/client` owns native transport and event cursor validation. The resources
extension schedules prompts, manages peer sessions and retains bounded routine delivery history.
Desktop client workflows in `crates/app/src/client` coordinate existing native projections and
configured launches. Mobile uses the public runtime RPC plus resources
commands through the shared native client; it does not depend on a running desktop GUI.

Session state (`idle`, `running`, `needs_input`, `cancelling`, `closed`, `failed`), message delivery
knowledge, a completed harness turn and a verified real-world task outcome are separate facts.
Harness-reported background tasks and semantic plans do not establish durable orchestration.

## Routine behavior enforced in this slice

Schedules are persisted in SQLite resources extension state and tick on the runtime host. They keep
working when all mobile clients close, as long as that host and runtime are running.

- One-shot schedule: an absolute UTC instant obtained from an ISO timestamp with explicit offset.
- Recurring schedule: first absolute instant plus a fixed interval of 1–31,536,000 elapsed seconds.
  It is not a wall-clock calendar rule and does not preserve an IANA timezone or daylight-saving time.
- Downtime: due occurrences coalesce into one delivery after restart. Missed intervals are not replayed
  in a burst. The next interval is calculated after delivery, so dispatch time can introduce drift.
- Overlap: a schedule's delivery phase is serialized. A busy agent receives a queued prompt; later
  recurrences can queue more work. There is no execution-level overlap prevention in this scheduler.
- Lifecycle: scheduled → running delivery → delivered/error; one-shot schedules disable afterward.
  Delivery errors do not establish native rejection or safe replay. Interrupted delivery disables the
  schedule for inspection. Simple resume cannot re-enable interrupted or cancelled records.
- Pause/resume: toggles future dispatch without rebuilding a session or deleting history. Resuming an
  overdue schedule causes the coalesced occurrence to become eligible.
- Cancellation: stops future occurrences and retains the schedule and its latest 64 attempts. It does
  not withdraw a queued prompt or cancel an accepted model turn. Active delivery cannot be cancelled
  through this schedule control; session controls govern active agent work.
- Attempts: stable attempt ID, start time, optional finish time, dispatch status and delivery result.
  State is persisted before dispatch. Restart retains uncertainty as interrupted. Updating a schedule
  preserves its prior history. This is delivery-attempt history, not a task-result ledger.

The existing queue path does not yet expose a fully durable occurrence/attempt/result handshake.
Consequently these routines must not be described as exactly-once work, recoverable delegated tasks
or execution-level overlap control. A queued/delivered prompt is not a completed task.

## Durable task boundary for the next runtime phase

The orchestrator must own durable work independently of session or phone lifetime. Use the existing
SQLite store for atomic task, attempt, lease, occurrence, wait and delivery records; another database,
message broker or workflow service is not required by current evidence.

A task needs stable task ID, owner scope, conversation ID, authored intent and idempotency key,
parent/delegation lineage, cancellation intent, completion criteria and explicit terminal evidence.
An execution attempt needs a distinct attempt ID, executor/session binding, fencing token, heartbeat/
lease, native submission key, dispatch knowledge and terminal model evidence. Separate task states
should include pending, ready, running, waiting for human, cancelling, succeeded, failed, cancelled and
requires inspection. Accepted delivery, terminal model turn and verified outcome remain separate
fields. A semantic plan describes intended steps; model decisions may propose updates but cannot
silently write a verified task-success transition.

The runtime must enforce legal transitions transactionally, claim ownership with leases and fencing,
record intent before dispatch, recover expired owners, reconcile native receipts before retry, and
refuse ambiguous replay. A restart must never infer failure merely because a worker disappeared.
Cancellation is a durable request followed by adapter acknowledgement or explicit uncertainty.
Human waits preserve the exact request/approval ID and decision, block automatic completion, and
resume only after an authorized answer. Adapters declare supported dispatch/status/cancel/resume
capabilities. They must not claim arbitrary internal harness state is serializable.

Delegation needs a stable child task and result-delivery ID, persisted outbox/inbox records,
parent/child ownership, acknowledgement/deduplication and cancellation policy. A parent must wait
for retained child evidence, rather than treating a send acknowledgement as its child's result.
A routine occurrence should create or reuse a task by `(routine ID, scheduled instant)` and define
skip, coalesce or bounded catch-up; overlap must explicitly be skip, queue or permitted parallelism.
Calendar recurrence needs a retained IANA zone, explicit DST gap/fold behavior and history that shows
both scheduled instant and actual attempt times. These capabilities are requirements for the next
phase, not implemented guarantees of the current scheduled-prompt extension.

## Deferred computer capability

`apps/mobile/src/computer/capability.ts` reserves display, input, clipboard, takeover and return
capabilities behind an adapter and an eventual route boundary. No route is registered, no controls
are exposed and no permissions or session connections are opened. A future WebView transport adapter
must be capability-gated and separately qualified; WebView's presence alone activates nothing.
