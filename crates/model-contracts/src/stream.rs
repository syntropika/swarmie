use crate::{Extension, ModelResponse, ProviderError};
use tembra::prelude::*;

/// These are inference deltas, not transport pings or durable execution transitions.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    crate = "tembra::serde",
    tag = "type",
    rename_all = "snake_case",
    deny_unknown_fields
)]
#[schemars(crate = "tembra::schemars", transform = crate::tagged_union)]
pub enum ModelEvent {
    TextDelta {
        text: String,
    },
    ReasoningDelta {
        text: String,
    },
    /// Replace the previously observed partial arguments for this call; never append them.
    ToolCallSnapshot {
        id: String,
        name: String,
        arguments_json: String,
    },
    Extension {
        extension: Extension,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct SequencedEvent {
    pub sequence: u64,
    pub event: ModelEvent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", rename_all = "snake_case")]
#[schemars(crate = "tembra::schemars")]
pub enum GenerationPhase {
    Prepared,
    Running,
    Settled,
}

/// A bounded, single-consumer window. Snapshot/reset delivery can skip publications: consume
/// events by sequence, then acknowledge a contiguous processed prefix. Terminal response is
/// authoritative for content block ordering and opaque continuation. Partial events survive errors.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct GenerationState {
    pub request_id: String,
    pub phase: GenerationPhase,
    pub acknowledged: u64,
    pub last_sequence: u64,
    pub events: Vec<SequencedEvent>,
    pub response: Option<ModelResponse>,
    pub error: Option<ProviderError>,
    /// True only after local HTTP future/stream has been dropped or consumed. Says nothing
    /// about remote provider computation or billing, even following cancellation/timeout.
    pub physically_settled: bool,
}
