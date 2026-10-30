use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::num::NonZeroU32;
use uuid::Uuid;

macro_rules! identity {
    ($name:ident) => {
        #[derive(
            Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}

identity!(TaskId);
identity!(ExecutorId);
identity!(AttemptId);
identity!(SignalKey);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum WaitFor {
    Delay { milliseconds: u64 },
    Signal(SignalKey),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum WaitState {
    Until(u64),
    Signal(SignalKey),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum TaskOutcome {
    Completed(Value),
    Waiting {
        checkpoint: Value,
        condition: WaitFor,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SignalReceipt {
    pub value: Value,
    pub consumed: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SignalDelivery {
    pub key: SignalKey,
    pub value: Value,
}

/// A versioned task service. The service exposes [`crate::TaskMethod::Execute`].
/// Each assignment gets an instance keyed by task ID and attempt ID. Its resources
/// are transient; progress that must survive a wait belongs in the checkpoint.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub struct TaskDefinition {
    service: String,
    pub version: NonZeroU32,
}

impl TaskDefinition {
    /// Creates a stable service identity, rejecting empty or unbounded names.
    ///
    /// ```
    /// use std::num::NonZeroU32;
    /// use swarmie_durable_tasks::TaskDefinition;
    /// let version = NonZeroU32::new(1).unwrap();
    /// let task = TaskDefinition::new("documents.process", version).unwrap();
    /// assert_eq!(task.service(), "documents.process");
    /// assert!(TaskDefinition::new("", version).is_err());
    /// ```
    pub fn new(
        service: impl Into<String>,
        version: NonZeroU32,
    ) -> Result<Self, crate::StorageError> {
        let service = service.into();
        if service.is_empty()
            || service.len() > 255
            || !service
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        {
            return Err(crate::StorageError::Invalid(
                "invalid task service name".into(),
            ));
        }
        Ok(Self { service, version })
    }

    pub fn service(&self) -> &str {
        &self.service
    }

    pub(crate) fn validate(&self) -> Result<(), crate::StorageError> {
        Self::new(self.service.clone(), self.version).map(|_| ())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TaskSpec {
    pub id: TaskId,
    pub definition: TaskDefinition,
    pub input: Value,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum TaskStatus {
    Ready,
    Waiting(WaitState),
    Completed(Value),
    Failed(TaskFailure),
}

/// Local execution observation, separate from the durable task status.
/// `Settled` means a terminal record has no assignment on this executor. It does
/// not prove that external effects or another executor have stopped.
/// `Interrupted` means this executor has joined the attempt and holds automatic
/// readmission until an explicit recovery decision. It is not a terminal result.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskObservation {
    Pending,
    Settled,
    Interrupted,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Assignment {
    pub executor: ExecutorId,
    pub attempt: AttemptId,
    /// Milliseconds from the storage authority's clock, not an executor deadline.
    pub expires_at: u64,
}

/// Durable data, distinct from the transient Tembra invocation and open resources.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TaskRecord {
    pub spec: TaskSpec,
    pub principal: String,
    pub checkpoint: Option<Value>,
    pub status: TaskStatus,
    /// Progress revision. Assignment acquisition, renewal and release do not change it.
    pub revision: u64,
    pub created_at: u64,
    pub sequence: u64,
    pub assignment: Option<Assignment>,
    pub delivery: Option<SignalDelivery>,
}

/// Typed terminal cause. Human messages are never used for control flow.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TaskFailure {
    #[serde(with = "failure_code")]
    pub code: tembra::Code,
    pub reason: String,
    pub message: String,
}

impl From<tembra::Error> for TaskFailure {
    fn from(error: tembra::Error) -> Self {
        Self {
            code: error.code,
            reason: error.reason,
            message: error.message,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum TaskState {
    Ready,
    Sleeping,
    Waiting,
    Completed,
    Failed,
}

impl TaskState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Sleeping => "sleeping",
            Self::Waiting => "waiting",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }
}

impl TaskStatus {
    pub fn state(&self) -> TaskState {
        match self {
            Self::Ready => TaskState::Ready,
            Self::Waiting(WaitState::Until(_)) => TaskState::Sleeping,
            Self::Waiting(WaitState::Signal(_)) => TaskState::Waiting,
            Self::Completed(_) => TaskState::Completed,
            Self::Failed(_) => TaskState::Failed,
        }
    }
}

/// Scheduling and observation metadata excludes input, checkpoint and result payloads.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TaskSummary {
    pub id: TaskId,
    pub definition: TaskDefinition,
    pub principal: String,
    pub revision: u64,
    pub sequence: u64,
    pub created_at: u64,
    pub state: TaskState,
    pub assignment: Option<Assignment>,
}

impl From<&TaskRecord> for TaskSummary {
    fn from(task: &TaskRecord) -> Self {
        Self {
            id: task.spec.id,
            definition: task.spec.definition.clone(),
            principal: task.principal.clone(),
            revision: task.revision,
            sequence: task.sequence,
            created_at: task.created_at,
            state: task.status.state(),
            assignment: task.assignment.clone(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TaskPageRequest {
    pub after: u64,
    pub limit: NonZeroU32,
}

impl TaskPageRequest {
    /// Maximum metadata rows returned by one public list request.
    pub const MAX_LIMIT: NonZeroU32 = NonZeroU32::new(1000).expect("nonzero constant");
}

impl Default for TaskPageRequest {
    fn default() -> Self {
        Self {
            after: 0,
            limit: NonZeroU32::new(100).expect("nonzero constant"),
        }
    }
}

#[derive(Clone)]
pub(crate) enum Transition {
    Checkpoint(Value),
    Complete(Value),
    Fail(TaskFailure),
}

impl TaskRecord {
    pub(crate) fn confirm(&mut self, transition: Transition) -> Result<(), crate::Rejection> {
        if self.status != TaskStatus::Ready {
            return Err(crate::Rejection::Terminal);
        }
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or(crate::Rejection::Exhausted)?;
        match transition {
            Transition::Checkpoint(value) => self.checkpoint = Some(value),
            Transition::Complete(value) => self.status = TaskStatus::Completed(value),
            Transition::Fail(value) => self.status = TaskStatus::Failed(value),
        }
        Ok(())
    }
}

mod failure_code {
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(code: &tembra::Code, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(code.as_str())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<tembra::Code, D::Error> {
        let value = String::deserialize(deserializer)?;
        tembra::Code::parse(&value)
            .ok_or_else(|| serde::de::Error::custom("unknown failure category"))
    }
}
