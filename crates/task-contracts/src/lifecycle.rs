//! Bounded observations over Tembra Hooks, separate from authoritative task state.
//!
//! One confirmed task write may produce several independent notifications. They are enqueued
//! in this order when applicable: created, phase started, checkpoint, waiting, sleeping,
//! recovered, blocked, parked, retry, cancellation requested, terminal. State-specific hooks
//! describe entry into that status; reconciliation of an unchanged status does not repeat them.
//! Checkpoint acceptance is observed even when its value is unchanged. A recovered write may
//! also enter parked, and a checkpoint write may also enter waiting or sleeping. A continuation
//! write reserves the next phase while accepting a checkpoint, so one revision reports phase
//! started followed by checkpoint, after the previous phase's settlement.
//!
//! Physical phase settlement is a separate fact from any subsequent handler-result commit.
//! Notification order is local to the publisher; it is not a global durable delivery order.
//! Each Registry uses Tembra series composition: one failing contribution skips the remaining
//! contributions in that invocation. Independent consumers requiring replay use TaskStorage's
//! durable transition outbox instead of relying on these best-effort notifications.

use tembra::{
    contracts::{Member, ValueSchema},
    prelude::*,
};
use tembra_hooks::{HookDescriptor, HookType, RustHookDefinition, Strategy};

use crate::{BlockingReason, TaskOutcome, TaskRecord, TaskStatus};

/// Stable metadata only. A committed revision identifies a transition within one task store.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct TaskLifecycleTask {
    #[schemars(length(min = 1, max = 128))]
    pub task_id: String,
    #[schemars(length(min = 1, max = 128))]
    pub task_kind: String,
    #[schemars(range(min = 1, max = 9_007_199_254_740_991_u64))]
    pub revision: u64,
    #[schemars(length(min = 1, max = 128))]
    pub phase: String,
    pub attempt: u32,
    #[schemars(range(min = 1))]
    pub definition_version: u32,
}

impl From<&TaskRecord> for TaskLifecycleTask {
    fn from(record: &TaskRecord) -> Self {
        Self {
            task_id: record.id.clone(),
            task_kind: record.kind.clone(),
            revision: record.revision,
            phase: record.phase.clone(),
            attempt: record.attempts,
            definition_version: record.definition_version,
        }
    }
}

/// The component operation that confirmed a write, independent of its resulting status.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", rename_all = "snake_case")]
#[schemars(crate = "tembra::schemars")]
pub enum TaskLifecycleOrigin {
    Creation,
    Reservation,
    Handler,
    Recovery,
    Cancellation,
    Retry,
    Reconciliation,
}

/// Managed invocation attempt kind; its receipt does not prove that handler entry occurred.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", rename_all = "snake_case")]
#[schemars(crate = "tembra::schemars")]
pub enum TaskPhaseKind {
    Run,
    Migrate,
}

/// Managed call result category, independent of later storage mutation.
/// `Rejected` includes admission refusal before handler entry. Cancellation signals alone
/// never prove that started work has physically finished.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", rename_all = "snake_case")]
#[schemars(crate = "tembra::schemars")]
pub enum TaskPhaseOutcome {
    Returned,
    Rejected,
    Cancelled,
}

/// Outcome category; full results and diagnostics remain in TaskStorage.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", rename_all = "snake_case")]
#[schemars(crate = "tembra::schemars")]
pub enum TaskOutcomeKind {
    Completed,
    Failed,
    Aborted,
    Faulted,
}

impl From<&TaskOutcome> for TaskOutcomeKind {
    fn from(outcome: &TaskOutcome) -> Self {
        match outcome {
            TaskOutcome::Completed { .. } => Self::Completed,
            TaskOutcome::Failed { .. } => Self::Failed,
            TaskOutcome::Aborted { .. } => Self::Aborted,
            TaskOutcome::Faulted { .. } => Self::Faulted,
        }
    }
}

/// Blocking category without provider details or unbounded diagnostic strings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", rename_all = "snake_case")]
#[schemars(crate = "tembra::schemars")]
pub enum TaskBlockingKind {
    MissingDefinition,
    IncompatibleDefinition,
    MigrationRejected,
    RestartLimit,
    HandlerFailure,
}

impl From<&BlockingReason> for TaskBlockingKind {
    fn from(reason: &BlockingReason) -> Self {
        match reason {
            BlockingReason::MissingDefinition => Self::MissingDefinition,
            BlockingReason::IncompatibleDefinition { .. } => Self::IncompatibleDefinition,
            BlockingReason::MigrationRejected { .. } => Self::MigrationRejected,
            BlockingReason::RestartLimit => Self::RestartLimit,
            BlockingReason::HandlerFailure { .. } => Self::HandlerFailure,
        }
    }
}

/// Best-effort observation, never a transactional write or durable delivery receipt.
///
/// `Committed` follows a confirmed storage receipt only. `PhaseSettled` identifies the
/// admitted reservation revision after the managed call receipt physically settles: no work
/// owned by that attempt remains. The receipt does not prove handler entry; a rejected attempt
/// may have been refused before entry. This fact never claims a later state commit.
/// A process can die between either fact and its notification; no replay
/// or exactly-once guarantee follows from receiving or missing a hook invocation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    crate = "tembra::serde",
    tag = "type",
    rename_all = "snake_case",
    deny_unknown_fields
)]
#[schemars(crate = "tembra::schemars", transform = crate::schema::tagged_union)]
pub enum TaskLifecycleEvent {
    Committed {
        task: TaskLifecycleTask,
        #[schemars(range(min = 1, max = 9_007_199_254_740_991_u64))]
        sequence: u64,
        previous_status: Option<TaskStatus>,
        current_status: TaskStatus,
        origin: TaskLifecycleOrigin,
        /// An accepted Checkpoint, Continue, Sleep, Wait or Migrate payload was committed,
        /// even unchanged.
        checkpoint_committed: bool,
        /// Cancellation intent changed from false to true in this confirmed write.
        cancellation_requested: bool,
        outcome: Option<TaskOutcomeKind>,
        blocking: Option<TaskBlockingKind>,
    },
    PhaseSettled {
        task: TaskLifecycleTask,
        invocation: TaskPhaseKind,
        outcome: TaskPhaseOutcome,
    },
}

/// Local notification counters; failures never alter task mutation results.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct TaskLifecycleDiagnostics {
    pub skipped: u64,
    pub enqueued: u64,
    pub completed: u64,
    pub failed: u64,
    pub dropped: u64,
    pub abandoned: u64,
    pub timed_out: u64,
    pub queued: u32,
    pub active: bool,
    #[schemars(length(max = 2048))]
    pub last_error: Option<String>,
}

/// Read-only local visibility into bounded best-effort notification delivery.
#[tembra::contract(local, name = "swarmie.task-lifecycle", version = 1)]
pub trait TaskLifecycle {
    #[portable]
    async fn diagnostics(&self) -> Result<TaskLifecycleDiagnostics>;
}

/// A confirmed storage transition, shared by the specialized lifecycle inputs.
///
/// The sequence is store-local. It is not a durable hook delivery receipt. Full checkpoints,
/// inputs, results and diagnostic strings remain in TaskStorage.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct TaskCommittedObservation {
    pub task: TaskLifecycleTask,
    #[schemars(range(min = 1, max = 9_007_199_254_740_991_u64))]
    pub sequence: u64,
    pub previous_status: Option<TaskStatus>,
    pub current_status: TaskStatus,
    pub origin: TaskLifecycleOrigin,
    pub checkpoint_committed: bool,
    pub cancellation_requested: bool,
    pub outcome: Option<TaskOutcomeKind>,
    pub blocking: Option<TaskBlockingKind>,
}

/// Creation confirmed by TaskStorage.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct TaskCreated {
    pub transition: TaskCommittedObservation,
}

/// An execution reservation was confirmed. Handler entry is not implied.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct TaskPhaseStarted {
    pub transition: TaskCommittedObservation,
}

/// An accepted checkpoint payload was committed, even if unchanged.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct TaskCheckpoint {
    pub transition: TaskCommittedObservation,
}

/// A confirmed transition entered Waiting.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct TaskWaiting {
    pub transition: TaskCommittedObservation,
}

/// A confirmed transition entered Sleeping.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct TaskSleeping {
    pub transition: TaskCommittedObservation,
}

/// A recovery transition was committed; no historical hook is replayed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct TaskRecovered {
    pub transition: TaskCommittedObservation,
}

/// A confirmed transition entered Blocked.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct TaskBlocked {
    pub transition: TaskCommittedObservation,
}

/// A confirmed transition entered Parked.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct TaskParked {
    pub transition: TaskCommittedObservation,
}

/// An explicit retry request was committed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct TaskRetry {
    pub transition: TaskCommittedObservation,
}

/// Cancellation intent changed from false to true in storage.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct TaskCancellationRequested {
    pub transition: TaskCommittedObservation,
}

/// A confirmed transition entered a terminal status.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct TaskTerminal {
    pub transition: TaskCommittedObservation,
}

/// Physical settlement of a managed invocation attempt, independent of state commits.
///
/// Rejected attempts may have been refused before handler entry. A cancellation signal alone
/// never proves settlement. This input has no storage sequence and is never an atomic outbox fact.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct TaskPhaseSettled {
    pub task: TaskLifecycleTask,
    pub invocation: TaskPhaseKind,
    pub outcome: TaskPhaseOutcome,
}

/// Schema export for the typed [`on_task_created`] input; not an observer service.
#[tembra::contract(local, name = "swarmie.task-created-observer-schema", version = 1)]
pub trait TaskCreatedObserver {
    #[portable]
    async fn observe(&self, event: TaskCreated) -> Result<()>;
}

/// Creation confirmed by TaskStorage.
///
/// Observers execute through Tembra's versioned series Registry. An error stops the remaining
/// handlers in this hook invocation; it never invalidates a confirmed storage transition.
/// Other lifecycle hooks produced by the same transition remain independent notifications.
///
/// # Errors
/// Refuses invalid canonical schema or descriptor metadata before invoking an observer.
pub fn on_task_created() -> Result<RustHookDefinition<TaskCreated, ()>> {
    series_hook(
        "swarmie.task-created",
        TaskCreatedObserver.descriptor()?.as_ref(),
    )
}

/// Schema export for the typed [`on_task_phase_started`] input; not an observer service.
#[tembra::contract(
    local,
    name = "swarmie.task-phase-started-observer-schema",
    version = 1
)]
pub trait TaskPhaseStartedObserver {
    #[portable]
    async fn observe(&self, event: TaskPhaseStarted) -> Result<()>;
}

/// An execution reservation was confirmed. Handler entry is not implied.
///
/// Observers execute through Tembra's versioned series Registry. An error stops the remaining
/// handlers in this hook invocation; it never invalidates a confirmed storage transition.
/// Other lifecycle hooks produced by the same transition remain independent notifications.
///
/// # Errors
/// Refuses invalid canonical schema or descriptor metadata before invoking an observer.
pub fn on_task_phase_started() -> Result<RustHookDefinition<TaskPhaseStarted, ()>> {
    series_hook(
        "swarmie.task-phase-started",
        TaskPhaseStartedObserver.descriptor()?.as_ref(),
    )
}

/// Schema export for the typed [`on_task_phase_settled`] input; not an observer service.
#[tembra::contract(
    local,
    name = "swarmie.task-phase-settled-observer-schema",
    version = 1
)]
pub trait TaskPhaseSettledObserver {
    #[portable]
    async fn observe(&self, event: TaskPhaseSettled) -> Result<()>;
}

/// Physical settlement of a managed invocation attempt.
///
/// Observers execute through Tembra's versioned series Registry. An error stops the remaining
/// handlers in this hook invocation; it never invalidates a confirmed storage transition.
/// Other lifecycle hooks produced by the same transition remain independent notifications.
///
/// # Errors
/// Refuses invalid canonical schema or descriptor metadata before invoking an observer.
pub fn on_task_phase_settled() -> Result<RustHookDefinition<TaskPhaseSettled, ()>> {
    series_hook(
        "swarmie.task-phase-settled",
        TaskPhaseSettledObserver.descriptor()?.as_ref(),
    )
}

/// Schema export for the typed [`on_task_checkpoint`] input; not an observer service.
#[tembra::contract(local, name = "swarmie.task-checkpoint-observer-schema", version = 1)]
pub trait TaskCheckpointObserver {
    #[portable]
    async fn observe(&self, event: TaskCheckpoint) -> Result<()>;
}

/// An accepted checkpoint payload was committed, even if unchanged.
///
/// Observers execute through Tembra's versioned series Registry. An error stops the remaining
/// handlers in this hook invocation; it never invalidates a confirmed storage transition.
/// Other lifecycle hooks produced by the same transition remain independent notifications.
///
/// # Errors
/// Refuses invalid canonical schema or descriptor metadata before invoking an observer.
pub fn on_task_checkpoint() -> Result<RustHookDefinition<TaskCheckpoint, ()>> {
    series_hook(
        "swarmie.task-checkpoint",
        TaskCheckpointObserver.descriptor()?.as_ref(),
    )
}

/// Schema export for the typed [`on_task_waiting`] input; not an observer service.
#[tembra::contract(local, name = "swarmie.task-waiting-observer-schema", version = 1)]
pub trait TaskWaitingObserver {
    #[portable]
    async fn observe(&self, event: TaskWaiting) -> Result<()>;
}

/// A confirmed transition entered Waiting.
///
/// Observers execute through Tembra's versioned series Registry. An error stops the remaining
/// handlers in this hook invocation; it never invalidates a confirmed storage transition.
/// Other lifecycle hooks produced by the same transition remain independent notifications.
///
/// # Errors
/// Refuses invalid canonical schema or descriptor metadata before invoking an observer.
pub fn on_task_waiting() -> Result<RustHookDefinition<TaskWaiting, ()>> {
    series_hook(
        "swarmie.task-waiting",
        TaskWaitingObserver.descriptor()?.as_ref(),
    )
}

/// Schema export for the typed [`on_task_sleeping`] input; not an observer service.
#[tembra::contract(local, name = "swarmie.task-sleeping-observer-schema", version = 1)]
pub trait TaskSleepingObserver {
    #[portable]
    async fn observe(&self, event: TaskSleeping) -> Result<()>;
}

/// A confirmed transition entered Sleeping.
///
/// Observers execute through Tembra's versioned series Registry. An error stops the remaining
/// handlers in this hook invocation; it never invalidates a confirmed storage transition.
/// Other lifecycle hooks produced by the same transition remain independent notifications.
///
/// # Errors
/// Refuses invalid canonical schema or descriptor metadata before invoking an observer.
pub fn on_task_sleeping() -> Result<RustHookDefinition<TaskSleeping, ()>> {
    series_hook(
        "swarmie.task-sleeping",
        TaskSleepingObserver.descriptor()?.as_ref(),
    )
}

/// Schema export for the typed [`on_task_recovered`] input; not an observer service.
#[tembra::contract(local, name = "swarmie.task-recovered-observer-schema", version = 1)]
pub trait TaskRecoveredObserver {
    #[portable]
    async fn observe(&self, event: TaskRecovered) -> Result<()>;
}

/// A recovery transition was committed; no historical hook is replayed.
///
/// Observers execute through Tembra's versioned series Registry. An error stops the remaining
/// handlers in this hook invocation; it never invalidates a confirmed storage transition.
/// Other lifecycle hooks produced by the same transition remain independent notifications.
///
/// # Errors
/// Refuses invalid canonical schema or descriptor metadata before invoking an observer.
pub fn on_task_recovered() -> Result<RustHookDefinition<TaskRecovered, ()>> {
    series_hook(
        "swarmie.task-recovered",
        TaskRecoveredObserver.descriptor()?.as_ref(),
    )
}

/// Schema export for the typed [`on_task_blocked`] input; not an observer service.
#[tembra::contract(local, name = "swarmie.task-blocked-observer-schema", version = 1)]
pub trait TaskBlockedObserver {
    #[portable]
    async fn observe(&self, event: TaskBlocked) -> Result<()>;
}

/// A confirmed transition entered Blocked.
///
/// Observers execute through Tembra's versioned series Registry. An error stops the remaining
/// handlers in this hook invocation; it never invalidates a confirmed storage transition.
/// Other lifecycle hooks produced by the same transition remain independent notifications.
///
/// # Errors
/// Refuses invalid canonical schema or descriptor metadata before invoking an observer.
pub fn on_task_blocked() -> Result<RustHookDefinition<TaskBlocked, ()>> {
    series_hook(
        "swarmie.task-blocked",
        TaskBlockedObserver.descriptor()?.as_ref(),
    )
}

/// Schema export for the typed [`on_task_parked`] input; not an observer service.
#[tembra::contract(local, name = "swarmie.task-parked-observer-schema", version = 1)]
pub trait TaskParkedObserver {
    #[portable]
    async fn observe(&self, event: TaskParked) -> Result<()>;
}

/// A confirmed transition entered Parked.
///
/// Observers execute through Tembra's versioned series Registry. An error stops the remaining
/// handlers in this hook invocation; it never invalidates a confirmed storage transition.
/// Other lifecycle hooks produced by the same transition remain independent notifications.
///
/// # Errors
/// Refuses invalid canonical schema or descriptor metadata before invoking an observer.
pub fn on_task_parked() -> Result<RustHookDefinition<TaskParked, ()>> {
    series_hook(
        "swarmie.task-parked",
        TaskParkedObserver.descriptor()?.as_ref(),
    )
}

/// Schema export for the typed [`on_task_retry`] input; not an observer service.
#[tembra::contract(local, name = "swarmie.task-retry-observer-schema", version = 1)]
pub trait TaskRetryObserver {
    #[portable]
    async fn observe(&self, event: TaskRetry) -> Result<()>;
}

/// An explicit retry request was committed.
///
/// Observers execute through Tembra's versioned series Registry. An error stops the remaining
/// handlers in this hook invocation; it never invalidates a confirmed storage transition.
/// Other lifecycle hooks produced by the same transition remain independent notifications.
///
/// # Errors
/// Refuses invalid canonical schema or descriptor metadata before invoking an observer.
pub fn on_task_retry() -> Result<RustHookDefinition<TaskRetry, ()>> {
    series_hook(
        "swarmie.task-retry",
        TaskRetryObserver.descriptor()?.as_ref(),
    )
}

/// Schema export for the typed [`on_task_cancellation_requested`] input; not an observer service.
#[tembra::contract(
    local,
    name = "swarmie.task-cancellation-requested-observer-schema",
    version = 1
)]
pub trait TaskCancellationRequestedObserver {
    #[portable]
    async fn observe(&self, event: TaskCancellationRequested) -> Result<()>;
}

/// Cancellation intent changed from false to true in storage.
///
/// Observers execute through Tembra's versioned series Registry. An error stops the remaining
/// handlers in this hook invocation; it never invalidates a confirmed storage transition.
/// Other lifecycle hooks produced by the same transition remain independent notifications.
///
/// # Errors
/// Refuses invalid canonical schema or descriptor metadata before invoking an observer.
pub fn on_task_cancellation_requested() -> Result<RustHookDefinition<TaskCancellationRequested, ()>>
{
    series_hook(
        "swarmie.task-cancellation-requested",
        TaskCancellationRequestedObserver.descriptor()?.as_ref(),
    )
}

/// Schema export for the typed [`on_task_terminal`] input; not an observer service.
#[tembra::contract(local, name = "swarmie.task-terminal-observer-schema", version = 1)]
pub trait TaskTerminalObserver {
    #[portable]
    async fn observe(&self, event: TaskTerminal) -> Result<()>;
}

/// A confirmed transition entered a terminal status.
///
/// Observers execute through Tembra's versioned series Registry. An error stops the remaining
/// handlers in this hook invocation; it never invalidates a confirmed storage transition.
/// Other lifecycle hooks produced by the same transition remain independent notifications.
///
/// # Errors
/// Refuses invalid canonical schema or descriptor metadata before invoking an observer.
pub fn on_task_terminal() -> Result<RustHookDefinition<TaskTerminal, ()>> {
    series_hook(
        "swarmie.task-terminal",
        TaskTerminalObserver.descriptor()?.as_ref(),
    )
}

fn series_hook<T: Serialize + 'static>(
    name: &str,
    contract: &tembra::contracts::Contract,
) -> Result<RustHookDefinition<T, ()>> {
    let Some(Member::Method {
        args: Some(ValueSchema::Portable(schema)),
        ..
    }) = contract
        .members("")
        .and_then(|members| members.get("observe"))
    else {
        return Err(RuntimeError::invalid(
            "task_lifecycle_schema",
            "observer schema must be portable",
        ));
    };
    let input = HookType::serde(schema.clone())?;
    let descriptor = HookDescriptor::new(
        name,
        1,
        Strategy::Series,
        input.profile().clone(),
        None,
        None,
    )?;
    RustHookDefinition::from_descriptor(descriptor, input, HookType::absent())
}
