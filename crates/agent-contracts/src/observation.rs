use tembra::prelude::*;

/// Bounded, volatile diagnostics for best-effort Agent Loop observations.
/// Counters saturate at the portable integer limit. No payloads or handler error messages
/// are retained. These observations do not establish durable execution or history.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct AgentObservationDiagnostics {
    pub enqueued: u64,
    pub completed: u64,
    pub failed: u64,
    pub timed_out: u64,
    pub dropped: u64,
    pub skipped: u64,
    pub abandoned: u64,
    pub queued: u32,
    pub queued_bytes: u32,
    pub active: bool,
    /// Bounded handler-provided reason identifier; message and payload are omitted.
    pub last_error_reason: Option<String>,
}

/// Read-only diagnostics of one live loop implementation. Host replacement resets counters;
/// SessionStore and TaskStorage remain the respective durable authorities.
#[tembra::contract(local, name = "swarmie.agent-observations", version = 1)]
pub trait AgentObservations {
    #[portable]
    async fn diagnostics(&self) -> Result<AgentObservationDiagnostics>;
}
