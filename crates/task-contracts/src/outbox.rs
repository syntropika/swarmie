//! Durable structural transition intent, independent of best-effort hook delivery.

use tembra::prelude::*;

use crate::{TaskBlockingKind, TaskOutcomeKind, TaskRecord, TaskStatus};

/// Maximum canonical JSON byte size of one durable structural event across providers.
pub const MAX_TASK_OUTBOX_EVENT_BYTES: usize = 4096;

/// Stable identity and total order within one TaskStorage instance. Ordinals are zero based
/// in the transaction's input order; sequences may contain multiple events.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct TaskOutboxEventId {
    #[schemars(length(min = 1, max = 128))]
    pub store_id: String,
    #[schemars(range(min = 1, max = 9_007_199_254_740_991_u64))]
    pub sequence: u64,
    #[schemars(range(max = 255))]
    pub ordinal: u32,
}

/// One confirmed record revision, persisted in the same transaction as that revision.
/// Metadata is derived from actual pre/post records. Changed checkpoint equality does not
/// claim that an unchanged checkpoint was not accepted. No input, checkpoint, output or free
/// diagnostic text is copied here, and physical handler settlement is never represented.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct TaskOutboxEvent {
    #[schemars(range(min = 1, max = 1))]
    pub version: u32,
    /// Total records in this atomic commit. Acknowledgement is per event; consumers that need
    /// atomic destination batches may buffer the complete group across bounded read pages.
    #[schemars(range(min = 1, max = 256))]
    pub transaction_size: u32,
    pub id: TaskOutboxEventId,
    #[schemars(length(min = 1, max = 128))]
    pub task_id: String,
    #[schemars(length(min = 1, max = 128))]
    pub task_kind: String,
    #[schemars(range(min = 1, max = 9_007_199_254_740_991_u64))]
    pub revision: u64,
    pub definition_version: u32,
    #[schemars(length(min = 1, max = 128))]
    pub phase: String,
    pub attempt: u32,
    pub restarts: u32,
    pub parent: Option<String>,
    pub previous_status: Option<TaskStatus>,
    pub current_status: TaskStatus,
    pub checkpoint_changed: bool,
    pub cancellation_requested: bool,
    pub outcome: Option<TaskOutcomeKind>,
    pub blocking: Option<TaskBlockingKind>,
}

impl TaskOutboxEvent {
    /// Computes structural truth after storage has validated the replacement record.
    pub fn from_records(
        id: TaskOutboxEventId,
        before: Option<&TaskRecord>,
        after: &TaskRecord,
        transaction_size: u32,
    ) -> Self {
        Self {
            version: 1,
            transaction_size,
            id,
            task_id: after.id.clone(),
            task_kind: after.kind.clone(),
            revision: after.revision,
            definition_version: after.definition_version,
            phase: after.phase.clone(),
            attempt: after.attempts,
            restarts: after.restarts,
            parent: after.parent.clone(),
            previous_status: before.map(|record| record.status),
            current_status: after.status,
            checkpoint_changed: before.is_none_or(|record| record.checkpoint != after.checkpoint),
            cancellation_requested: after.cancellation_requested
                && before.is_none_or(|record| !record.cancellation_requested),
            outcome: after.outcome.as_ref().map(TaskOutcomeKind::from),
            blocking: after.blocking_reason.as_ref().map(TaskBlockingKind::from),
        }
    }
}

/// Durable consumer identity. Registering an existing ID is idempotent. Registration starts at
/// the earliest retained event, not necessarily the beginning of storage history after compaction.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct TaskOutboxConsumer {
    pub id: String,
}

/// Durable consumer position and explicit compacted history boundary. An empty store still
/// exposes its identity. New consumers start at `retained_after`; earlier history is unavailable.
/// The boundary may also describe historical commits predating outbox installation; ordinal 255
/// denotes the end of such an unavailable commit sequence, without inventing past events.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct TaskOutboxConsumerState {
    pub consumer_id: String,
    #[schemars(length(min = 1, max = 128))]
    pub store_id: String,
    pub acknowledged: Option<TaskOutboxEventId>,
    pub delivered: Option<TaskOutboxEventId>,
    pub retained_after: Option<TaskOutboxEventId>,
}

/// Append-stable pagination. None reads after the durable acknowledgement. A continuation
/// must be a previously delivered event for this consumer; new commits never invalidate it.
/// Read and publish serially per consumer identity. Multiple workers require separate identities
/// or external coordination. Pages never exceed the common portable task page byte budget.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct TaskOutboxQuery {
    pub consumer_id: String,
    pub after: Option<TaskOutboxEventId>,
    pub limit: u32,
}

/// `next` is the last returned identity, including at the current tail. Reusing it polls for
/// appended events. An empty page retains the supplied cursor. Publish before acknowledging.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct TaskOutboxPage {
    #[schemars(length(min = 1, max = 128))]
    pub store_id: String,
    pub retained_after: Option<TaskOutboxEventId>,
    pub events: Vec<TaskOutboxEvent>,
    pub next: Option<TaskOutboxEventId>,
    pub acknowledged: Option<TaskOutboxEventId>,
}

/// Acknowledges an already delivered prefix. Repeating an acknowledgement is idempotent;
/// skipping beyond the delivered high water mark or using another store's identity is refused.
/// A crash after publication before this durable write causes redelivery: consumers must dedupe.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct TaskOutboxAcknowledgement {
    pub consumer_id: String,
    pub through: TaskOutboxEventId,
}

/// Explicit bounded cleanup; only rows acknowledged by every current consumer are eligible.
/// No consumers means retain everything. Removing a consumer explicitly relinquishes its
/// retention protection; registration alone never retroactively restores compacted history.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct TaskOutboxCompaction {
    pub limit: u32,
}

/// Current retained usage and the number removed by this cleanup call.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct TaskOutboxRetention {
    pub removed: u32,
    pub retained_events: u64,
    pub retained_bytes: u64,
}
