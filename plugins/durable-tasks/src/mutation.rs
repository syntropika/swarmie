//! Compact mutation provenance; authoritative records come only from storage receipts.

use swarmie_task_contracts::{
    TaskLifecycleEvent, TaskLifecycleOrigin, TaskLifecycleTask, TaskRecord, TaskStatus, TaskWrite,
};

/// A small before-image avoids rereading storage or retaining task inputs and checkpoints.
#[derive(Clone, Copy)]
pub(crate) struct Change {
    previous_status: Option<TaskStatus>,
    previous_cancellation: bool,
    pub origin: TaskLifecycleOrigin,
    pub checkpoint_committed: bool,
}

impl Change {
    pub fn from_record(record: &TaskRecord, origin: TaskLifecycleOrigin) -> Self {
        Self {
            previous_status: Some(record.status),
            previous_cancellation: record.cancellation_requested,
            origin,
            checkpoint_committed: false,
        }
    }

    pub fn event(self, record: &TaskRecord, sequence: u64) -> TaskLifecycleEvent {
        TaskLifecycleEvent::Committed {
            task: TaskLifecycleTask::from(record),
            sequence,
            previous_status: self.previous_status,
            current_status: record.status,
            origin: self.origin,
            checkpoint_committed: self.checkpoint_committed,
            cancellation_requested: record.cancellation_requested && !self.previous_cancellation,
            outcome: record.outcome.as_ref().map(Into::into),
            blocking: record.blocking_reason.as_ref().map(Into::into),
        }
    }
}

pub(crate) struct Mutation {
    pub write: TaskWrite,
    pub change: Change,
}

impl Mutation {
    pub fn create(record: TaskRecord) -> Self {
        Self {
            write: TaskWrite {
                expected_revision: None,
                record,
            },
            change: Change {
                previous_status: None,
                previous_cancellation: false,
                origin: TaskLifecycleOrigin::Creation,
                checkpoint_committed: false,
            },
        }
    }
}
