use tembra::prelude::*;

/// An opaque, case-sensitive identifier containing 1..=128 ASCII identifier bytes.
pub type TaskId = String;

/// JSON-safe integers are bounded to avoid precision loss across portable runtimes.
pub const MAX_PORTABLE_INTEGER: u64 = 9_007_199_254_740_991;

/// Initial serializable task state. IDs are caller-supplied so submission can be reconciled.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct TaskSpec {
    pub id: TaskId,
    pub kind: String,
    pub definition_version: u32,
    pub input: serde_json::Value,
    pub checkpoint: serde_json::Value,
    pub phase: String,
}

/// Durable lifecycle, distinct from Tembra Reference readiness or execution settlement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", rename_all = "snake_case")]
#[schemars(crate = "tembra::schemars")]
pub enum TaskStatus {
    Pending,
    Running,
    Waiting,
    Sleeping,
    Completing,
    Blocked,
    Parked,
    Terminal,
}

/// Decided outcome; completing tasks retain this while owned foreground children settle.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    crate = "tembra::serde",
    tag = "type",
    rename_all = "snake_case",
    deny_unknown_fields
)]
#[schemars(crate = "tembra::schemars", transform = crate::schema::tagged_union)]
pub enum TaskOutcome {
    Completed { value: serde_json::Value },
    Failed { error: String },
    Aborted { reason: String },
    Faulted { error: String },
}

/// Waiting observes direct owned foreground children only, preventing cross-tree wait cycles.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", rename_all = "snake_case")]
#[schemars(crate = "tembra::schemars")]
pub enum WaitPolicy {
    #[default]
    AllSettled,
    FailFast,
}

/// Inspectable scheduler diagnostic; records and checkpoints remain intact while blocked.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    crate = "tembra::serde",
    tag = "type",
    rename_all = "snake_case",
    deny_unknown_fields
)]
#[schemars(crate = "tembra::schemars", transform = crate::schema::tagged_union)]
pub enum BlockingReason {
    MissingDefinition,
    IncompatibleDefinition { target_version: u32, detail: String },
    MigrationRejected { target_version: u32, detail: String },
    RestartLimit,
    HandlerFailure { detail: String },
}

/// Complete authoritative record. No executable handles, implementation IDs or closures persist.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct TaskRecord {
    pub id: TaskId,
    pub kind: String,
    pub definition_version: u32,
    pub input: serde_json::Value,
    pub checkpoint: serde_json::Value,
    pub phase: String,
    pub status: TaskStatus,
    pub revision: u64,
    pub parent: Option<TaskId>,
    pub wait_for: Vec<TaskId>,
    pub wait_policy: WaitPolicy,
    pub wake_at_ms: Option<u64>,
    pub outcome: Option<TaskOutcome>,
    pub cancellation_requested: bool,
    pub attempts: u32,
    pub restarts: u32,
    pub blocking_reason: Option<BlockingReason>,
}

impl TaskRecord {
    /// Creates a pending record at revision one. Storage validates the complete transaction.
    pub fn new(spec: TaskSpec, parent: Option<TaskId>) -> Self {
        Self {
            id: spec.id,
            kind: spec.kind,
            definition_version: spec.definition_version,
            input: spec.input,
            checkpoint: spec.checkpoint,
            phase: spec.phase,
            status: TaskStatus::Pending,
            revision: 1,
            parent,
            wait_for: vec![],
            wait_policy: WaitPolicy::AllSettled,
            wake_at_ms: None,
            outcome: None,
            cancellation_requested: false,
            attempts: 0,
            restarts: 0,
            blocking_reason: None,
        }
    }

    /// True only after all required owned work has settled durably.
    pub fn is_terminal(&self) -> bool {
        self.status == TaskStatus::Terminal
    }
}

/// Validates stable identities without database-specific collation or normalization.
pub fn validate_identity(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.:".contains(&byte))
    {
        return Err(RuntimeError::invalid(
            "task_invalid_identity",
            "identifiers require 1..=128 ASCII letters, digits, hyphens, underscores, dots or colons",
        ));
    }
    Ok(())
}
