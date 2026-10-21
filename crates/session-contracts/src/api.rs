use tembra::prelude::*;

use crate::{EventPage, EventQuery, NewEvent, ProjectionPage, ProjectionQuery};

/// Idempotent create identity. Repeating the same complete request returns the existing record;
/// reusing its ID with other metadata is refused. Metadata is immutable portable JSON.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct CreateSession {
    pub session_id: String,
    pub metadata: serde_json::Value,
}

/// Exact identity, valid across Host replacement when the database remains the same.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct SessionLookup {
    pub session_id: String,
}

/// Current committed state, including the durable head for replay and optimistic concurrency.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct SessionRecord {
    pub store_id: String,
    pub session_id: String,
    pub head: u64,
    pub metadata: serde_json::Value,
}

/// Small committed notification hint. Subscribe with Tembra State, then catch up from durable
/// per-session cursors. It is not an event delivery queue or proof that every delta was observed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct StoreHead {
    pub store_id: String,
    pub commit_sequence: u64,
}

/// Atomic ordered append, limited to 128 events. Batch identity is unique within one session.
/// An exact retry returns the original receipt before evaluating expected_head. Changed events,
/// order, or expected_head under the same batch ID are refused. Duplicate event IDs under a new
/// batch ID are refused, including exact duplicates; retry the original batch after uncertainty.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct AppendEvents {
    pub session_id: String,
    pub batch_id: String,
    pub expected_head: Option<u64>,
    pub events: Vec<NewEvent>,
}

/// Stable receipt, identical on retry even if later appends have advanced the session.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct AppendReceipt {
    pub store_id: String,
    pub session_id: String,
    pub batch_id: String,
    pub first_sequence: u64,
    pub last_sequence: u64,
    pub commit_sequence: u64,
}

/// Provider-neutral history authority. Only successful storage commits establish progress.
/// Validation, CAS, duplicate and size refusals mutate nothing. An I/O, publication, transport
/// failure or cancelled caller may have unknown outcome: retry the *identical* batch identity.
/// A native provider replacement requires a fresh Host; closing never deletes history.
/// There is no transaction spanning TaskStorage and this Contract.
#[tembra::contract(local, name = "swarmie.session-store", version = 1)]
pub trait SessionStore {
    #[state]
    fn committed(&self) -> StoreHead;

    #[portable]
    async fn create(&self, request: CreateSession) -> Result<SessionRecord>;

    /// Opens current durable state. Absence is explicit; storage failures are never absence.
    #[portable]
    async fn open(&self, request: SessionLookup) -> Result<Option<SessionRecord>>;

    #[portable]
    async fn append(&self, request: AppendEvents) -> Result<AppendReceipt>;

    /// Reads immutable event pages, including unknown kinds and future payload versions.
    #[portable]
    async fn events(&self, query: EventQuery) -> Result<EventPage>;

    /// Bounded pure projections of a fixed source prefix. No inference or agent policy runs.
    #[portable]
    async fn project(&self, query: ProjectionQuery) -> Result<ProjectionPage>;
}
