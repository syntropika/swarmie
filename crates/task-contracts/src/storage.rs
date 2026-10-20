use tembra::prelude::*;

use crate::{TaskId, TaskRecord, TaskStatus};

/// Exact task identity, without implementation-specific lookup details.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct TaskLookup {
    pub id: TaskId,
}

/// A continuation is valid only while the global committed sequence is unchanged.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct TaskCursor {
    pub store_id: String,
    pub sequence: u64,
    pub after: TaskId,
    pub statuses: Vec<TaskStatus>,
    pub parent: Option<TaskId>,
}

/// Bounded UTF-8 byte ordered scan. A missing parent filter includes roots and children.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct TaskQuery {
    pub limit: u32,
    pub cursor: Option<TaskCursor>,
    pub statuses: Vec<TaskStatus>,
    pub parent: Option<TaskId>,
}

impl Default for TaskQuery {
    fn default() -> Self {
        Self {
            limit: 128,
            cursor: None,
            statuses: vec![],
            parent: None,
        }
    }
}

/// Committed page. An invalidated cursor is an explicit error, never a mixed snapshot.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct TaskPage {
    pub sequence: u64,
    pub records: Vec<TaskRecord>,
    pub next: Option<TaskCursor>,
}

/// Compare-and-swap of one complete record. None requires absence and record revision one;
/// Some(n) requires existing revision n and replacement revision n+1. No deletion operation exists.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct TaskWrite {
    pub expected_revision: Option<u64>,
    pub record: TaskRecord,
}

/// Atomic batch, limited to 256 distinct tasks. Relationships are validated against its final
/// state. Parent identity, kind and input are immutable; terminal records cannot be rewritten.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct TaskTransaction {
    pub owner: String,
    pub writes: Vec<TaskWrite>,
}

/// A revision conflict commits nothing. Other validation, storage, corruption and closed-resource
/// errors are explicit Tembra RuntimeErrors with stable task_storage_* reasons.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    crate = "tembra::serde",
    tag = "type",
    rename_all = "snake_case",
    deny_unknown_fields
)]
#[schemars(crate = "tembra::schemars", transform = crate::schema::tagged_union)]
pub enum CommitResult {
    Committed {
        sequence: u64,
        records: Vec<TaskRecord>,
    },
    Conflict {
        id: TaskId,
        actual_revision: Option<u64>,
    },
}

/// Ephemeral exclusive scheduler admission. This is neither a lease nor an execution generation.
/// A provider permits one claimed owner until release or Resource closure. Claiming the same ID
/// is idempotent; a different live owner, stale release or unclaimed commit is refused.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct StorageOwner {
    pub id: String,
}

/// Portable transactional storage boundary. It never chooses handlers, retries or cancellation
/// policy. Implementations retain committed data on retirement and enforce one active owner
/// boundary per database. Successful writes are durable; an I/O error can have unknown outcome,
/// so callers must read records before deciding whether to retry an uncertain commit.
#[tembra::contract(local, name = "swarmie.task-storage", version = 1)]
pub trait TaskStorage {
    #[portable]
    async fn claim_owner(&self, owner: StorageOwner) -> Result<()>;

    #[portable]
    async fn release_owner(&self, owner: StorageOwner) -> Result<()>;

    #[portable]
    async fn get(&self, request: TaskLookup) -> Result<Option<TaskRecord>>;

    #[portable]
    async fn scan(&self, query: TaskQuery) -> Result<TaskPage>;

    #[portable]
    async fn commit(&self, transaction: TaskTransaction) -> Result<CommitResult>;
}
