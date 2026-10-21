//! Durable structural transition intent, independent of best-effort hook delivery.

use tembra::prelude::*;

use crate::{TaskBlockingKind, TaskLifecycleOrigin, TaskOutcomeKind, TaskRecord, TaskStatus};

/// Maximum canonical JSON byte size of one durable structural event across providers.
pub const MAX_TASK_OUTBOX_EVENT_BYTES: usize = 4096;

/// Maximum UTF-8 bytes in a stable installed Facet name used for consumer ownership.
pub const MAX_TASK_OUTBOX_OWNER_BYTES: usize = 256;

/// Checks an outbox owner's bounded installed Facet name without truncation.
///
/// # Errors
/// Refuses empty, oversized, or control-containing names before provider mutation.
pub fn validate_task_outbox_owner_facet(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > MAX_TASK_OUTBOX_OWNER_BYTES
        || name.chars().any(char::is_control)
    {
        return Err(RuntimeError::new(
            tembra::core::ErrorCode::Forbidden,
            "task_storage_outbox_owner_invalid",
            "consumer owners require 1..=256 UTF-8 bytes without control characters",
        ));
    }
    Ok(())
}

/// Derives consumer authority only from Tembra's immediate managed caller identity.
///
/// # Errors
/// Unattributed Host/RPC roots are refused. Principal values and request fields confer no
/// authority. The name must satisfy [`validate_task_outbox_owner_facet`].
pub fn task_outbox_consumer_owner(context: &tembra::Context) -> Result<String> {
    let owner = context.caller_facet().ok_or_else(|| {
        RuntimeError::new(
            tembra::core::ErrorCode::Forbidden,
            "task_storage_outbox_owner_required",
            "consumer operations require an attributed managed Facet caller",
        )
    })?;
    validate_task_outbox_owner_facet(owner)?;
    Ok(owner.to_owned())
}

/// Hard retained capacity, applied by the provider independently of scheduler admission.
/// Missing JSON fields use defaults. No expiry, eviction, or acknowledgement is implied.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", default, deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct TaskOutboxLimits {
    #[schemars(range(min = 1, max = 9_007_199_254_740_991_u64))]
    pub max_events: u64,
    #[schemars(range(min = 1, max = 9_007_199_254_740_991_u64))]
    pub max_bytes: u64,
    #[schemars(range(min = 1, max = 64))]
    pub max_consumers: u32,
}

impl Default for TaskOutboxLimits {
    fn default() -> Self {
        Self {
            max_events: 1_000_000,
            max_bytes: 1_073_741_824,
            max_consumers: 64,
        }
    }
}

impl TaskOutboxLimits {
    /// Checks bounded admission settings without reading or changing storage.
    ///
    /// # Errors
    /// Rejects zero or nonportable byte/event counts and consumer limits outside 1..=64.
    pub fn validate(self) -> Result<()> {
        if self.max_events == 0
            || self.max_events > crate::MAX_PORTABLE_INTEGER
            || self.max_bytes == 0
            || self.max_bytes > crate::MAX_PORTABLE_INTEGER
            || !(1..=64).contains(&self.max_consumers)
        {
            return Err(RuntimeError::invalid(
                "task_storage_invalid",
                "outbox limits require positive portable event and byte counts and 1..=64 consumers",
            ));
        }
        Ok(())
    }
}

/// Read-only coherent outbox snapshot, available independently of scheduler ownership and
/// capacity. Consumers are ordered by their case-sensitive identifier and never exceed 64.
/// `compactable_events` is the retained prefix acknowledged by every registered consumer;
/// it is zero with no consumers. `pending_events` counts the remaining protected events.
/// Retained byte counts measure canonical event JSON, not database/index/WAL disk usage.
/// Reading status never registers a consumer, advances delivery, acknowledges, or deletes data.
/// Computing the compactable count may traverse the acknowledged retained prefix; providers
/// should expose this monitoring snapshot outside their scheduling hot path.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct TaskOutboxStatus {
    #[schemars(length(min = 1, max = 128))]
    pub store_id: String,
    pub limits: TaskOutboxLimits,
    pub retained_after: Option<TaskOutboxEventId>,
    pub retained_events: u64,
    pub retained_bytes: u64,
    pub compactable_events: u64,
    pub pending_events: u64,
    #[schemars(length(max = 64))]
    pub consumers: Vec<TaskOutboxConsumerState>,
}

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

/// Bounded operation provenance reported by the claimed task writer.
///
/// Storage atomically retains this annotation after ownership, CAS and record validation.
/// It does not independently prove handler entry, physical execution or checkpoint acceptance.
/// `checkpoint_committed` means the writer accepted a checkpoint operation in this write,
/// including an equal checkpoint value; `TaskOutboxEvent::checkpoint_changed` measures equality.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct TaskTransitionProvenance {
    pub origin: TaskLifecycleOrigin,
    pub checkpoint_committed: bool,
}

/// One confirmed record revision, persisted in the same transaction as that revision.
/// Structural metadata is derived from actual pre/post records. Optional provenance records
/// the claimed writer's operation annotation, without independently verifying execution.
/// Version 1 has no provenance; version 2 may omit it when the operation is unknown. Historical
/// events retain their original version, identity and canonical payload; readers never backfill.
/// No input, checkpoint, output or free diagnostic text is copied here, and physical handler
/// settlement is never represented.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct TaskOutboxEvent {
    #[schemars(range(min = 1, max = 2))]
    pub version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<TaskTransitionProvenance>,
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
        provenance: Option<TaskTransitionProvenance>,
    ) -> Self {
        Self {
            version: 2,
            provenance,
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

/// Durable consumer identity, bound on first registration to Tembra's managed caller Facet.
/// Another Facet cannot register/resume, read, acknowledge, or remove that identity. Direct Host
/// and RPC roots are unattributed and refused. No caller-provided owner or bearer token exists.
/// Registering an existing ID by its owner is idempotent. Registration starts at
/// the earliest retained event, not necessarily the beginning of storage history after compaction.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct TaskOutboxConsumer {
    #[schemars(length(min = 1, max = 128))]
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
    /// Informational installed Facet name, never consumption authority. None marks a legacy
    /// unowned row, which retains history and refuses use until explicit operator adoption.
    #[schemars(length(min = 1, max = 256))]
    pub owner_facet: Option<String>,
    #[schemars(length(min = 1, max = 128))]
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
    #[schemars(length(min = 1, max = 128))]
    pub consumer_id: String,
    pub after: Option<TaskOutboxEventId>,
    #[schemars(range(min = 1, max = 256))]
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
    #[schemars(length(min = 1, max = 128))]
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
    #[schemars(range(min = 1, max = 4096))]
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
