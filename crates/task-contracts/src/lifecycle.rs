//! Bounded observations over Tembra Hooks, separate from authoritative task state.

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
        /// An accepted Checkpoint, Sleep, Wait or Migrate payload was committed, even unchanged.
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

/// Schema export for generated DTOs, not a runtime observer service.
/// Contributors use the exact managed method from [`task_lifecycle`]'s descriptor instead.
#[tembra::contract(local, name = "swarmie.task-lifecycle-observer-schema", version = 1)]
pub trait TaskLifecycleObserver {
    #[portable]
    async fn observe(&self, event: TaskLifecycleEvent) -> Result<()>;
}

/// Read-only local visibility into bounded best-effort notification delivery.
#[tembra::contract(local, name = "swarmie.task-lifecycle", version = 1)]
pub trait TaskLifecycle {
    #[portable]
    async fn diagnostics(&self) -> Result<TaskLifecycleDiagnostics>;
}

/// Canonical typed series pipeline over Tembra's existing owned Registry operations.
///
/// # Errors
/// Refuses invalid generated schema or descriptor metadata before invoking any observer.
pub fn task_lifecycle() -> Result<RustHookDefinition<TaskLifecycleEvent, ()>> {
    let contract = TaskLifecycleObserver.descriptor()?;
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
        "swarmie.task-lifecycle",
        1,
        Strategy::Series,
        input.profile().clone(),
        None,
        None,
    )?;
    RustHookDefinition::from_descriptor(descriptor, input, HookType::absent())
}
