//! Recoverable task execution through ordinary Tembra orbs and shared storage ownership.

mod authority;
mod definitions;
mod execution;
mod model;
mod orb;
mod scheduler;
mod storage;
mod transitions;

pub use definitions::Definitions;
pub use execution::Execution;
pub use model::{
    Assignment, AttemptId, ExecutorId, SignalDelivery, SignalKey, SignalReceipt, TaskDefinition,
    TaskFailure, TaskId, TaskOutcome, TaskPageRequest, TaskRecord, TaskSpec, TaskState, TaskStatus,
    TaskSummary, WaitFor, WaitState,
};
pub use orb::{HOST_PRINCIPAL, INSTANCE_KEY, Method, SERVICE, TaskMethod, orb};
pub use scheduler::{
    Fifo, Options, SchedulerDiagnostic, SchedulerStatus, SchedulerStrategy, Timer,
};
pub use storage::{
    AssignmentGrant, CandidateSnapshot, CommitOutcome, PreparedTasks, Rejection, SignalReceipts,
    SignalSnapshot, StorageError, StorageFactory, TaskChange, TaskStorage, required_receipts,
};

pub use transitions::apply_changes;
