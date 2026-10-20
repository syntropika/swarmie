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
