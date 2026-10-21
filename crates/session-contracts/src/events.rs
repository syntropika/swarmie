use tembra::prelude::*;

/// Optional causal identifiers. The containing event supplies session and event identity.
/// Identifiers are opaque, case-sensitive, and never interpreted as credentials or paths.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct Correlation {
    pub conversation_id: Option<String>,
    pub turn_id: Option<String>,
    pub task_id: Option<String>,
    pub step_id: Option<String>,
    pub attempt_id: Option<String>,
    pub parent_event_id: Option<String>,
}

/// Versioned, extensible event. Unknown kinds and future payload versions remain readable.
/// Producers must redact secrets and exclude internal reasoning not legitimately exposed by
/// the provider. Authorized opaque continuation data may be retained verbatim. Token deltas
/// should be aggregated into bounded messages or explicit checkpoints before append.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct NewEvent {
    pub event_id: String,
    pub kind: String,
    pub version: u32,
    pub correlation: Correlation,
    /// Application timestamp, not the ordering authority; absent timestamps are valid.
    pub occurred_at_ms: Option<u64>,
    pub payload: serde_json::Value,
}

/// A stored event is immutable. Sequence starts at one and is contiguous within a session.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct SessionEvent {
    pub session_id: String,
    pub sequence: u64,
    pub event: NewEvent,
}

/// Provider-neutral transcript data, not a model request. Content is portable structured JSON;
/// model adapters own its translation to model contracts. Message identity equals event_id.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct Message {
    /// User, assistant, tool, or an application-defined speaker role.
    pub role: String,
    pub content: serde_json::Value,
    pub reply_to: Option<String>,
    pub tool_call_id: Option<String>,
}

/// An instruction change is retained at its original sequence and scope.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct Instructions {
    pub scope: String,
    pub content: serde_json::Value,
}

/// Context policy annotation. It never rewrites source events or silently replaces context.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct Compaction {
    pub through_sequence: u64,
    pub policy: String,
    pub summary: serde_json::Value,
}

/// External immutable content descriptor. No fetch, credential storage or blob lifecycle is
/// implied. Producers publish content durably before referencing it and retain it with history.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct BlobReference {
    pub uri: String,
    pub sha256: String,
    pub bytes: u64,
    pub media_type: String,
}

/// Known transcript kind. Technical/plugin kinds are open namespaced strings.
pub const MESSAGE: &str = "swarmie.message";

/// Known instruction kind.
pub const INSTRUCTIONS: &str = "swarmie.instructions";

/// Known compaction annotation kind.
pub const COMPACTION: &str = "swarmie.compaction";
