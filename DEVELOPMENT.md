# Developing Swarmie

This guide covers local setup, development commands, extension points and repository structure.
For an introduction to the project, see [README.md](README.md).

## Architecture

The runtime bootstraps Tembra and plugin administration. Application capabilities live in
independently composable plugins:

- **Plugin Manager** handles installation, activation, recovery and administrative persistence.
- **MemoryStore** provides namespaced persistent JSON storage.
- **Durable Tasks** handles scheduling, checkpoints, recovery, ownership and cancellation.
- **Task Storage SQLite** implements transactional task persistence behind the TaskStorage Contract.
- **Application plugins** contribute their own capabilities and registered task handlers.

Core Plugins are Rust Facets compiled with Swarmie. Dynamic Plugins are trusted TypeScript or
JavaScript artifacts loaded through Tembra. Tembra owns their execution, dependency readiness,
Resources and compatible JavaScript replacement.

Durable Tasks separates execution from persistence. TaskStorage commits are authoritative;
reactive State exposes committed progress. Checkpoints preserve resumable state, and atomic
transactions can update a parent while creating its children. Applications use stable operation
identities and idempotency to handle external effects that may repeat after interruption.

### Task storage and transport limits

TaskStorage bounds complete records to 768 KiB and complete portable pages, transactions,
receipts and handler invocations to 1 MiB minus 8 KiB. These are canonical UTF-8 JSON byte
limits, including envelopes and escaped strings. The record validator reserves 8 KiB for
control data and budgets mutable counters at their maximum encoded widths so checkpoint
data cannot consume the space needed for recovery diagnostics.
SQLite persists the same canonical JSON number representation used by the portable boundary;
mathematically equivalent values such as `1.0` and `1` do not change immutable input identity.

A scan page can contain fewer records than its requested limit. Follow its continuation
cursor until `next` is absent; record count alone does not indicate the end of a scan.
Cursors bind the provider, filters and committed sequence. Concurrent writes can invalidate
a continuation, requiring a fresh consistent read. The scheduler retries transient read
contention without treating it as a storage outage. Oversized values fail explicitly with
`task_storage_too_large` before storage mutation. Values rejected before method admission
retain Tembra's portable transport errors. Checkpoints and child lists are never silently
truncated. Shared validation helpers live in
[`crates/task-contracts/src/transport.rs`](crates/task-contracts/src/transport.rs).

### Recovery and retention

Terminal records are immutable and retained indefinitely, including outcomes and ownership
relationships. Automatic deletion is not part of TaskStorage. Indexed active-state and
parent queries keep unrelated terminal history outside scheduling work; applications can
inspect retained history through bounded graph queries. Plan storage capacity for retained
data and backups. Portable revision and global commit counters are finite. Exhaustion refuses
further writes explicitly; counters never wrap or reset to bypass compare-and-swap checks.

Interrupted Running reservations are recovered when a new scheduler owner starts. A known
commit conflict after a handler has settled parks the task with its latest durable checkpoint
and an explicit diagnostic. Cancellation intent takes precedence over an uncommitted handler
decision. Before retrying a parked effect, reconcile the external operation using its stable
identity; a rejected local transition does not prove that the external effect did not happen.

## Durable task lifecycle extensions

Durable Tasks defines twelve independent version-one Tembra `series` Hooks. Each has a typed
Rust Contract, its own Registry and generated TypeScript payload. Subscribe to the Hooks your
plugin needs rather than filtering a shared event stream:

| TypeScript Hook | Rust definition | Meaning |
| --- | --- | --- |
| `onTaskCreated` | `on_task_created()` | A new task record was committed. |
| `onTaskPhaseStarted` | `on_task_phase_started()` | A Running reservation was committed; handler entry is not implied. |
| `onTaskPhaseSettled` | `on_task_phase_settled()` | The tracked invocation physically settled, independently of any result commit. |
| `onTaskCheckpoint` | `on_task_checkpoint()` | A checkpoint operation was accepted and committed, including permitted equal values. |
| `onTaskWaiting` | `on_task_waiting()` | A committed transition entered Waiting. |
| `onTaskSleeping` | `on_task_sleeping()` | A committed transition entered Sleeping. |
| `onTaskRecovered` | `on_task_recovered()` | Crash recovery committed a new revision. |
| `onTaskBlocked` | `on_task_blocked()` | A committed transition entered Blocked. |
| `onTaskParked` | `on_task_parked()` | A committed transition entered Parked. |
| `onTaskRetry` | `on_task_retry()` | An explicit retry was committed. |
| `onTaskCancellationRequested` | `on_task_cancellation_requested()` | Cancellation intent became durable. |
| `onTaskTerminal` | `on_task_terminal()` | A committed transition entered Terminal. |

Rust definitions and DTOs are exported by `swarmie-task-contracts`; TypeScript definitions and
payloads are exported by [`plugins/durable-tasks/typescript.ts`](plugins/durable-tasks/typescript.ts).
Generate the bindings and executable fixtures with `just tembra-plugins` before typechecking
plugins. The canonical schemas live in
[`crates/task-contracts/src/lifecycle.rs`](crates/task-contracts/src/lifecycle.rs).

A Rust observer contributes its owned method through the Hook definition:

```rust
let hook = swarmie_task_contracts::on_task_terminal()?;
hook.register(declaration, "audit.task-terminal", 0, "observe")?;
```

The method uses Tembra's typed `HookArgs<TaskTerminal, ()>` and `hook_handler`; its observed
transition is `args.input().transition`. The observer owns its Contract and method. See the
[real native observer fixture](tests/plugins/fixtures/durable-tasks/src/hooks.rs) for complete
Facet declarations and physical invocation ownership.

A TypeScript observer declares a local member using `onTaskTerminal.method`, registers it with
`registerHook(self, onTaskTerminal, "audit.task-terminal", "observe", { priority: 0 })`, and
implements it through `hookHandler(onTaskTerminal, handler)`. The handler receives the typed
`input.transition`. The [TypeScript observer fixture](tests/plugins/fixtures/durable-tasks/observer.ts)
shows complete registration and compatible replacement. Hooks are registered through Tembra;
plugins must not create a separate listener registry.

The common committed payload carries bounded task identity, revision, phase, attempt, sequence,
prior/current status, operation origin, checkpoint acceptance and outcome/blocking categories.
It excludes full input, checkpoint and result bodies. Phase settlement instead describes the
admitted Run/Migrate invocation and Returned/Rejected/Cancelled result category. A returned
handler value does not prove that a task transition was valid or committed.

When several notifications describe one commit, admission order is Created, PhaseStarted,
Checkpoint, Waiting, Sleeping, Recovered, Blocked, Parked, Retry, CancellationRequested, Terminal.
For example, a checkpoint can also enter Waiting or Sleeping; recovery can also enter Parked;
cancellation intent can accompany Terminal. Physical settlement is independently observed before
the subsequent result-commit notifications. Unchanged reconciliation does not repeat status-entry
notifications. Concurrent callers do not obtain a globally durable Hook delivery order.

Tembra determines ascending priority, stable tie order, contribution availability, cancellation
and physical settlement. `series` fails fast within the current Hook invocation; the dispatcher
records the failure and continues later notifications after settlement. A failing observer cannot
undo a confirmed task commit. The shared queue admits at most 256 notifications, one physical
run and 64 contributions per run. At capacity it drops new observations and records diagnostics;
a multi-Hook notification set can therefore be partially admitted. With no handlers, nothing is
enqueued. `TaskLifecycle.diagnostics()` exposes skips, failures, drops, deadlines and abandonment.

The one-second observation deadline requests cancellation and still joins physical work.
Non-cooperative handlers can delay other observations and Host closure. Compatible JavaScript
reload retains admitted original-generation work; later plans select replacement contributions.
Retirement removes future contributions. Process death can lose queued notifications; Hooks do
not replay missing history automatically and do not provide exactly-once external delivery.

### Durable transition history and outbox operation

TaskStorage atomically commits each record revision with a bounded structural outbox event.
Its identity is `(store_id, sequence, ordinal)`; `transaction_size` describes the atomic group
across bounded pages. Failed CAS, validation or capacity admission produces no event. Physical
phase settlement is not an atomic task-state event. Record snapshots cannot reconstruct lost
intermediate revisions, so use the outbox for reliable publication rather than Hook delivery.

New event version two can also retain optional `TaskTransitionProvenance`: the committing
writer's operation origin and whether it accepted a checkpoint. `checkpoint_changed` remains
an actual value comparison; acceptance can be true when equality is unchanged. Provenance is
writer-reported committed metadata, not independent verification that a handler executed.
Direct writes may omit it. Version-one history remains readable with unknown provenance;
existing event identities and canonical content are not backfilled or rewritten.

A durable consumer must run under its own Tembra Facet identity and retain that identity across
restart or compatible reload. Reading, acknowledging and retiring another Facet's consumer is
refused; knowing a consumer ID is insufficient. Unattributed roots cannot claim a consumer.
`owner_facet` reports the durable owner; it is `null` only for unassigned historical consumers.
Use ordinary owned References and managed Facet/activity contexts for publisher operations.
This is local component attribution, not remote authentication or a sandbox against native code
that holds the Host or another live Context. Uninstalling a Facet does not relinquish its
consumer's retention protection. Preserve the installed Facet name across publisher versions.

Unassigned historical consumers refuse consumption and protect their unacknowledged prefix.
Existing durable acknowledgements remain authoritative. An operator can
explicitly adopt them while opening the Resource using the SQLite plugin's JSON
`outbox_legacy_owners` map, for example `{ "session-history": "app.session-publisher" }`.
The map is bounded to 64 entries and owner names to 256 UTF-8 bytes. Adoption applies only to
unassigned rows; a missing consumer or an existing different owner refuses opening. It never
silently claims a consumer, overwrites an owner, acknowledges history or deletes events.

Register required consumers before starting task production; late registration starts at the
retained boundary and cannot recover already compacted history.

The publisher owns the complete drain loop: read, idempotently commit each destination event,
acknowledge only successful durable receipts, and call `compact_outbox()` in bounded batches.
**Acknowledgement alone does not release capacity.** Run the publisher in an independent
Tembra-owned activity so it remains runnable when Durable Tasks exhausts scheduling slots.
A crash after publication before acknowledgement causes redelivery; deduplicate using the full
source event identity and preserve the exact destination request across retries. Serialize workers
sharing a consumer ID, including overlapping reload generations. Destination
session routing and mapping versions must be durable. The Session publisher is a separate
integration; this runtime does not infer routes or create an automatic drain daemon.

`TaskStorage.outbox_status()` reports active limits, retained event/JSON byte counts, protected
pending events, compactable events and durable consumer positions. Poll outside the scheduling
hot path. Alert before capacity exhaustion, on a missing required consumer, stalled acknowledgement,
publication failures and retained usage growing despite acknowledgements. If
`compactable_events` grows while retained usage stays high, check the publisher's compaction loop.
Monitor physical database/WAL size and filesystem free space separately from logical JSON bytes.

The SQLite plugin's `outbox` JSON settings control `max_events`, `max_bytes` and
`max_consumers`, defaulting to 1,000,000 events, 1 GiB and 64 consumers. Omitted fields use defaults.
Change settings and reopen the same database in a fresh Host to increase capacity without
rebuilding or deleting history. There is no live resize API, implicit expiry or silent eviction;
no consumers means retain all events. A full outbox refuses the entire task transaction.

### Cancellation during capacity exhaustion

A successful `DurableTasks.cancel()` confirms durable cancellation intent, or returns an already
terminal record. Confirmed intent takes precedence over uncommitted handler decisions.
If outbox capacity definitely rejects the intent write, the scheduler still signals current
active work locally and returns `durable_tasks_cancellation_unconfirmed`, including failed
ancestry checks. It does not invent `cancellation_requested`, an outbox event or a cancellation
Hook. Descendants are signalled only when their ownership can be established. Arbitrary
non-cooperative code still requires physical settlement.

Restore capacity and retry cancellation to make it durable. A restart may resume work whose
local cancellation was never confirmed. Capacity rejection after a handler has already settled
retains that decision within its managed execution instead of invoking the external effect again;
a subsequent crash still follows the task's existing replay/idempotency rules.

Compiled Core Plugins declare typed language adapters with
`CompiledPlugin.with_language_adapter(...)`. The Runtime registers compiled definitions before
source/application activation and cached Dynamic recovery. The bootstrap and Plugin Manager do
not need a new capability-specific branch for each Hook family. Future agent/tool Hooks belong
to their defining capability and may use typed transformation policies; task lifecycle observers
return no transaction replacement.

The canonical [TaskStorage contracts](crates/task-contracts/src/storage.rs) and
[outbox payloads](crates/task-contracts/src/outbox.rs) define provider requirements and bounds.

## Getting started

Install [mise](https://mise.jdx.dev/), Rustup, Python 3.11 or later, Git and your platform's
native build tools. Run development commands from the repository root:

```sh
mise install
mise exec -- bun install --frozen-lockfile
mise exec -- just tembra-sdk
```

Tool versions are pinned in [`mise.toml`](mise.toml). The Tembra SDK is built from the same
immutable public revision as the Rust dependencies. Public builds require no private
repository checkout or hosted credentials.

```sh
# Website
mise exec -- just dev

# Shared web components
mise exec -- just storybook

# Mobile development
mise exec -- just dev-mobile
```

Application Profiles select plugins and their configuration. The runtime can inspect an
application configuration before deployment:

```sh
mise exec -- just rust-run -p swarmie-runtime -- resolve application.json
```

See the [composition types](crates/composition/src/model.rs) and
[application API](crates/runtime/src/application.rs) for configuration and embedding.

## Extending Swarmie

Define application capabilities as Tembra Facets with explicit Contracts. Use Registries for
lifecycle-owned contributions and Resources for provider connections.

Rust Contracts are canonical. The build generates TypeScript bindings under
`target/generated/contracts/`; generated bindings and executable artifacts stay outside
version control.

- [Plugin Contracts](crates/plugin-contracts/src/lib.rs)
- [TaskStorage Contract](crates/task-contracts/src/storage.rs)
- [Durable Tasks and handler Contracts](crates/task-contracts/src/tasks.rs)
- [TypeScript task registration](plugins/durable-tasks/typescript.ts)

## Repository layout

| Location | Contents |
| --- | --- |
| `crates/` | Runtime, composition infrastructure and public Contracts. |
| `plugins/` | Application capabilities and storage providers. |
| `apps/web/` | Website. |
| `apps/mobile/` | Mobile application. |
| `packages/ui/` | Shared web and native UI packages. |
| `packages/config/` | Shared frontend configuration, fonts and design tokens. |
| `assets/brandkit/` | Brand assets. |
| `tests/plugins/` | Plugin fixtures and Host integration tests. |

## Development checks

```sh
mise exec -- just check
```

This runs formatting, typechecks, lint, frontend tests, plugin generation, Rust workspace tests,
Clippy and Rustdoc checks. `mise exec -- just tembra-authoring-test` regenerates fixtures and
checks Rust/TypeScript Hook descriptor parity. Additional recipes are listed by
`mise exec -- just --list`.
