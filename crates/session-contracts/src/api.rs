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

/// Immutable identity of the database served by the selected SessionStore provider.
///
/// It equals the `store_id` of session records, event pages, cursors, append receipts and
/// committed watermarks. It names the store that the live provider serves at the time of the
/// call; it is not a durability or liveness proof. Consumers that detect store replacement must
/// ask the current provider again instead of reusing an earlier answer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct SessionStoreIdentity {
    pub store_id: String,
}

/// Bounded observation of reactive repair. SQLite results remain authoritative while pending.
/// The last error retains only its category; failures include repeated managed repair attempts.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct PublicationStatus {
    pub durable: StoreHead,
    pub published: StoreHead,
    pub pending: bool,
    pub failures: u64,
    pub last_error: Option<String>,
}

/// Atomic ordered append, limited to 128 events. Batch identity is unique within one session.
/// An exact retry returns the original receipt before evaluating expected_head. Changed events,
/// order, or expected_head under the same batch ID are refused. Duplicate event IDs under a new
/// batch ID are refused, including exact duplicates; retry the original batch after uncertainty.
/// Payload depth reserves the complete event-page and projection envelopes, so every admitted
/// event remains portable when read back. A deeper payload is refused before any mutation.
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
/// Validation, CAS, duplicate and size refusals mutate nothing. An I/O or transport
/// failure or cancelled caller may have unknown outcome: retry the *identical* batch identity.
/// A native provider replacement requires a fresh Host; closing never deletes history.
/// Successful SQLite reads and commits do not depend on reactive publication. Publication lag
/// is observable separately and repaired through managed activity while the Host remains live.
/// There is no transaction spanning TaskStorage and this Contract.
#[tembra::contract(local, name = "swarmie.session-store", version = 1)]
pub trait SessionStore {
    #[state]
    fn committed(&self) -> StoreHead;

    /// Returns the immutable store identity loaded when the provider's storage opened. It reads
    /// no history, never reconciles publication and never changes state. A closed provider
    /// refuses instead of answering. Use it for identity checks; `publication_status` remains
    /// a diagnostic.
    #[portable]
    async fn identity(&self) -> Result<SessionStoreIdentity>;

    /// Reads publication lag and bounded failure categories without requiring State adoption.
    /// Its durable `store_id` equals `identity`; prefer that cheaper method for identity checks.
    #[portable]
    async fn publication_status(&self) -> Result<PublicationStatus>;

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
