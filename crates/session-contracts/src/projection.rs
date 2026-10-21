use tembra::prelude::*;

use crate::{COMPACTION, INSTRUCTIONS, MESSAGE, SessionEvent};

/// Durable cursor for a fixed immutable prefix. Later appends never invalidate it. To tail,
/// start a new query after the completed prefix's through_sequence with through=None.
/// Conversation and turn selectors are part of cursor identity; changing either is refused.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct EventCursor {
    pub store_id: String,
    pub session_id: String,
    pub after_sequence: u64,
    pub through_sequence: u64,
    pub conversation_id: Option<String>,
    pub turn_id: Option<String>,
}

/// Row and canonical byte bounds apply together. Without a cursor, through=None freezes the
/// current head. Conversation and turn filters combine with AND and preserve source sequence
/// identity and prefix bounds. An absent conversation selects the entire session, including
/// uncorrelated events. Multiplexed model contexts must select their conversation explicitly.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct EventQuery {
    pub session_id: String,
    pub after_sequence: u64,
    pub through_sequence: Option<u64>,
    pub conversation_id: Option<String>,
    pub turn_id: Option<String>,
    pub limit: u32,
    pub max_bytes: u32,
    pub cursor: Option<EventCursor>,
}

impl EventQuery {
    /// Starts a bounded full-history scan. Preserve `next` in subsequent requests.
    pub fn new(session_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            after_sequence: 0,
            through_sequence: None,
            conversation_id: None,
            turn_id: None,
            limit: 128,
            max_bytes: crate::MAX_PAGE_BYTES as u32,
            cursor: None,
        }
    }
}

/// A complete prefix boundary and a lossless continuation. Empty filtered pages can still
/// advance through_sequence. A budget too small for one row returns an explicit size error.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct EventPage {
    pub store_id: String,
    pub session_id: String,
    pub through_sequence: u64,
    pub events: Vec<SessionEvent>,
    pub next: Option<EventCursor>,
}

/// Version-one projections retain original identity and correlation for every included entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", rename_all = "snake_case")]
#[schemars(crate = "tembra::schemars")]
pub enum ProjectionKind {
    Conversation,
    Trajectory,
    ModelContext,
}

/// Source pagination is independent of presentation: never advance by projected item count.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct ProjectionQuery {
    pub source: EventQuery,
    pub kind: ProjectionKind,
}

/// Readable transcript entries carry typed message/instruction/compaction payloads in their
/// event data. Trajectory includes every recorded fact, including technical/plugin events.
/// Model context is a neutral ordered message/instruction stream for an agent adapter, not a
/// model request. It applies no compaction, retrieval, dynamic prompt, filter or token budget.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct ProjectionPage {
    pub projection_version: u32,
    pub kind: ProjectionKind,
    pub store_id: String,
    pub session_id: String,
    pub through_sequence: u64,
    pub entries: Vec<SessionEvent>,
    /// Source events excluded by this projection, including unsupported transcript versions.
    pub omitted: u32,
    /// Unrecognized kinds or versions. Consumers requiring semantic completeness must resolve
    /// these from the source log before constructing a model request.
    pub uninterpreted: u32,
    pub next: Option<EventCursor>,
}

/// Replays a source page deterministically with bounded memory. Original events are unchanged;
/// unknown kinds/versions remain available in trajectory and the authoritative event log.
/// Cross-page state is unnecessary: order, causal IDs and compaction ranges are preserved.
pub fn project(page: EventPage, kind: ProjectionKind) -> ProjectionPage {
    let count = page.events.len();
    let uninterpreted = page
        .events
        .iter()
        .filter(|entry| {
            entry.event.version != 1
                || !matches!(
                    entry.event.kind.as_str(),
                    MESSAGE | INSTRUCTIONS | COMPACTION
                )
        })
        .count() as u32;
    let entries: Vec<_> = page
        .events
        .into_iter()
        .filter(|entry| match kind {
            ProjectionKind::Trajectory => true,
            ProjectionKind::Conversation => {
                entry.event.version == 1
                    && matches!(
                        entry.event.kind.as_str(),
                        MESSAGE | INSTRUCTIONS | COMPACTION
                    )
            }
            ProjectionKind::ModelContext => {
                entry.event.version == 1
                    && matches!(entry.event.kind.as_str(), MESSAGE | INSTRUCTIONS)
            }
        })
        .collect();
    ProjectionPage {
        projection_version: 1,
        kind,
        store_id: page.store_id,
        session_id: page.session_id,
        through_sequence: page.through_sequence,
        omitted: (count - entries.len()) as u32,
        entries,
        uninterpreted,
        next: page.next,
    }
}
