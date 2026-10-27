use crate::{Assignment, AttemptId, ExecutorId, TaskId, TaskRecord, TaskSpec};
use futures::future::LocalBoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, num::NonZeroU64, rc::Rc};

/// One bounded ready-work read using the authority clock.
#[derive(Clone, Debug)]
pub struct CandidateSnapshot {
    pub now: u64,
    pub candidates: Vec<crate::TaskSummary>,
    pub next_wake: Option<u64>,
}

#[derive(Clone, Debug)]
pub struct AssignmentGrant {
    pub id: TaskId,
    pub assignment: Assignment,
}

/// Task-domain changes evaluated in order inside one authority transaction.
/// All checks succeed before any change is confirmed; partial batches are never applied.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TaskChange {
    Suspend {
        id: TaskId,
        attempt: AttemptId,
        revision: u64,
        checkpoint: Value,
        condition: crate::WaitFor,
    },
    Signal {
        id: TaskId,
        key: crate::SignalKey,
        value: Value,
    },
    Create {
        spec: TaskSpec,
        principal: String,
    },
    Acquire {
        id: TaskId,
        revision: u64,
        executor: ExecutorId,
        attempt: AttemptId,
        lease_ms: NonZeroU64,
    },
    Renew {
        id: TaskId,
        attempt: AttemptId,
        lease_ms: NonZeroU64,
    },
    Checkpoint {
        id: TaskId,
        attempt: AttemptId,
        revision: u64,
        checkpoint: Value,
    },
    Complete {
        id: TaskId,
        attempt: AttemptId,
        revision: u64,
        result: Value,
    },
    Fail {
        id: TaskId,
        attempt: AttemptId,
        revision: u64,
        error: crate::TaskFailure,
    },
    Release {
        id: TaskId,
        attempt: AttemptId,
    },
}

/// Confirmed refusal of the whole batch. It neither implies an unknown acknowledgement
/// nor authorizes immediate repeated retries of a permanently invalid request.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, thiserror::Error)]
pub enum Rejection {
    #[error("task already exists")]
    AlreadyExists,
    #[error("task does not exist")]
    NotFound,
    #[error("task revision changed")]
    Revision,
    #[error("task has a valid assignment")]
    Assigned,
    #[error("attempt has lost commit authority")]
    StaleAttempt,
    #[error("task is terminal")]
    Terminal,
    #[error("numeric capacity exhausted")]
    Exhausted,
    #[error("invalid task data")]
    Invalid,
    #[error("required receipt state was not loaded")]
    IncompleteReceipts,
}

#[derive(Clone, Debug)]
pub enum CommitOutcome {
    Applied { grants: Vec<AssignmentGrant> },
    Rejected(Rejection),
    Unknown { message: String },
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum StorageError {
    #[error(transparent)]
    Owner(#[from] swarmie_storage_core::OwnerError),
    #[error("domain write refused: {0}")]
    Refused(tembra::Error),
    #[error("task storage is closed")]
    Closed,
    #[error("task storage is unavailable: {0}")]
    Unavailable(String),
    #[error("invalid stored task data: {0}")]
    Invalid(String),
}

/// Capability of a storage owner, without assuming SQL or a universal records model.
pub trait TaskStorage {
    fn owner(&self) -> swarmie_storage_core::StorageOwner;

    /// Local committed eligibility invalidation. Other-authority changes require
    /// the host adapter to call wake; notices are hints, never assignment authority.
    fn changes(&self) -> swarmie_storage_core::ChangeSource;

    /// Reads exactly one task, including payload, when a consumer requests it.
    fn get(&self, id: TaskId) -> LocalBoxFuture<'_, Result<Option<TaskRecord>, StorageError>>;

    /// Bounded metadata page; no result, input or checkpoint payloads are decoded.
    fn list(
        &self,
        principal: Option<String>,
        page: crate::TaskPageRequest,
    ) -> LocalBoxFuture<'_, Result<Vec<crate::TaskSummary>, StorageError>>;

    /// Reads one bounded page of eligible scheduling metadata using the authority clock.
    fn candidates(
        &self,
        after: u64,
        limit: usize,
    ) -> LocalBoxFuture<'_, Result<CandidateSnapshot, StorageError>>;

    /// Validates and applies the complete batch atomically. Refusal changes nothing;
    /// unknown acknowledgements freeze the common owner, across domain capabilities.
    fn commit(
        &self,
        changes: Vec<TaskChange>,
    ) -> LocalBoxFuture<'_, Result<CommitOutcome, StorageError>>;

    /// Confirms domain preparation and task changes in one transaction of this owner.
    /// Adapters without joint-commit support refuse nonempty preparation before writing.
    fn commit_with(
        &self,
        changes: Vec<TaskChange>,
        prepared: swarmie_storage_core::PreparedChanges,
    ) -> LocalBoxFuture<'_, Result<CommitOutcome, StorageError>> {
        Box::pin(async move {
            let commands = prepared
                .into_changes(&self.owner())
                .map_err(StorageError::Refused)?;
            if !commands.is_empty() {
                return Err(StorageError::Refused(tembra::Error::new(
                    tembra::Code::Unsupported,
                    "swarmie.storage.joint_commit",
                    "adapter does not support domain preparation",
                )));
            }
            self.commit(changes).await
        })
    }

    /// Releases this capability lease after its users settle. The backend closes
    /// the shared owner only after its last capability lease has been released.
    fn close(&self) -> LocalBoxFuture<'_, Result<(), StorageError>>;
}

pub type StorageFactory =
    Rc<dyn Fn() -> LocalBoxFuture<'static, Result<Rc<dyn TaskStorage>, StorageError>>>;

/// Targeted persisted receipt changes, committed with the task records.
pub type SignalReceipts = BTreeMap<(TaskId, crate::SignalKey), crate::SignalReceipt>;

/// Loaded receipt values. A present `None` confirms absence; a missing key is unloaded.
pub type SignalSnapshot = BTreeMap<(TaskId, crate::SignalKey), Option<crate::SignalReceipt>>;

/// Keys an adapter must read inside the same authority transaction as task state.
pub fn required_receipts(
    changes: &[TaskChange],
) -> std::collections::BTreeSet<(TaskId, crate::SignalKey)> {
    changes.iter().filter_map(TaskChange::receipt_key).collect()
}

impl TaskChange {
    pub(super) fn receipt_key(&self) -> Option<(TaskId, crate::SignalKey)> {
        match self {
            Self::Signal { id, key, .. }
            | Self::Suspend {
                id,
                condition: crate::WaitFor::Signal(key),
                ..
            } => Some((*id, *key)),
            _ => None,
        }
    }

    pub fn id(&self) -> TaskId {
        match self {
            Self::Create { spec, .. } => spec.id,
            Self::Acquire { id, .. }
            | Self::Suspend { id, .. }
            | Self::Signal { id, .. }
            | Self::Renew { id, .. }
            | Self::Checkpoint { id, .. }
            | Self::Complete { id, .. }
            | Self::Fail { id, .. }
            | Self::Release { id, .. } => *id,
        }
    }
}

/// Candidate state and monotonic FIFO sequence; adopt neither on a refused batch.
#[derive(Clone, Debug, PartialEq)]
pub struct PreparedTasks {
    pub records: BTreeMap<TaskId, TaskRecord>,
    pub sequence: u64,
    pub signals: SignalReceipts,
}
