use tembra::{core, prelude::*};

use crate::{TaskLookup, TaskOutcome, TaskPage, TaskQuery, TaskRecord, TaskSpec, WaitPolicy};

/// Lifecycle-owned Registry metadata. One unambiguous operation is registered per task kind.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct TaskDefinition {
    pub kind: String,
    pub version: u32,
}

/// Canonical portable arguments for an application-owned registered step operation.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(
    crate = "tembra::serde",
    tag = "type",
    rename_all = "snake_case",
    deny_unknown_fields
)]
#[schemars(crate = "tembra::schemars", transform = crate::schema::tagged_union)]
pub enum TaskInvocation {
    Run {
        task: TaskRecord,
        children: Vec<TaskRecord>,
    },
    Migrate {
        task: TaskRecord,
        target_version: u32,
    },
}

/// Explicit next durable transition. A successful handler invocation is never itself progress.
/// Commit a stable operation ID in a checkpoint before performing a repeatable external effect
/// in the next phase. The scheduler commits this transition only after the invocation settles.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(
    crate = "tembra::serde",
    tag = "type",
    rename_all = "snake_case",
    deny_unknown_fields
)]
#[schemars(crate = "tembra::schemars", transform = crate::schema::tagged_union)]
pub enum TaskTransition {
    Checkpoint {
        checkpoint: serde_json::Value,
        phase: String,
    },
    /// Opt-in continuation of the same managed execution. One Running write commits this
    /// checkpoint and phase and reserves that phase as a new attempt, with restarts reset and
    /// no blocking reason; the handler is then invoked again for it. The write reports a phase
    /// start followed by a checkpoint.
    ///
    /// The scheduler commits ordinary `checkpoint` semantics instead, leaving the task Pending,
    /// when the definition resolved at the next phase's fresh Context is unavailable or has
    /// another version (blocking or migration then runs first), when another runnable task waits
    /// while every execution slot is occupied, or when the attempt counter is exhausted.
    /// Cancellation intent and newer records still take precedence over the settled decision.
    /// The checkpoint must describe durable progress, and a crash during the continued phase
    /// recovers it like any other interrupted reservation.
    Continue {
        checkpoint: serde_json::Value,
        phase: String,
    },
    Sleep {
        checkpoint: serde_json::Value,
        phase: String,
        wake_at_ms: u64,
    },
    Wait {
        checkpoint: serde_json::Value,
        phase: String,
        children: Vec<TaskSpec>,
        task_ids: Vec<String>,
        policy: WaitPolicy,
    },
    Complete {
        value: serde_json::Value,
    },
    Fail {
        error: String,
    },
    Abort {
        reason: String,
    },
    Migrate {
        checkpoint: serde_json::Value,
        phase: String,
    },
    RejectMigration {
        reason: String,
    },
}

/// Canonical method schema only. Each application serves its own named Contract with an
/// identical portable step method, then registers that method in [`task_definitions`].
/// This avoids multiple application providers competing for one singleton Contract identity.
#[tembra::contract(local, name = "swarmie.task-handler-schema", version = 1)]
pub trait TaskHandler {
    #[portable]
    async fn step(&self, invocation: TaskInvocation) -> Result<TaskTransition>;
}

/// Actual Tembra Registry declaration shared by scheduler and contributing application Facets.
pub fn task_definitions() -> Result<core::Registry> {
    core::Registry::new("swarmie.task-definitions", core::ConflictPolicy::Error)
}

/// Result of a bounded scheduler reconciliation request. Automatic scheduling uses the same path.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct ResumeResult {
    pub progressed: u32,
}

/// Committed inspection and lifecycle-independent durable submission boundary.
#[tembra::contract(local, name = "swarmie.durable-tasks", version = 1)]
pub trait DurableTasks {
    /// Last observed successful storage commit; graph reads carry their own consistent sequence.
    #[state]
    fn committed_sequence(&self) -> u64;

    /// Creates an independent root task. Owned children are created by a Wait transition.
    #[portable]
    async fn create(&self, task: TaskSpec) -> Result<TaskRecord>;

    #[portable]
    async fn get(&self, request: TaskLookup) -> Result<Option<TaskRecord>>;

    /// Suspends the caller until a terminal committed outcome; does not retain a task phase.
    #[portable]
    async fn wait(&self, request: TaskLookup) -> Result<TaskOutcome>;

    /// Records logical intent and propagates it to owned work. Does not force external effects
    /// or arbitrary non-cooperative handler code to stop.
    ///
    /// Success confirms persisted intent (or an already terminal task). If outbox capacity
    /// definitely rejects that write, `durable_tasks_cancellation_unconfirmed` reports only
    /// local cancellation signals to current invocations and any failed descendant checks.
    /// No durable intent or transition is implied by that error; restore capacity and retry.
    /// A process restart can resume the task unless cancellation was subsequently confirmed.
    #[portable]
    async fn cancel(&self, request: TaskLookup) -> Result<TaskRecord>;

    #[portable]
    async fn retry(&self, request: TaskLookup) -> Result<TaskRecord>;

    #[portable]
    async fn resume(&self) -> Result<ResumeResult>;

    /// Reads bounded committed graph records with the same consistent cursor semantics as storage.
    #[portable]
    async fn graph(&self, query: TaskQuery) -> Result<TaskPage>;
}
