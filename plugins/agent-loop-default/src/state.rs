use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use swarmie_agent_contracts::{StartTurn, TurnContext};
use tembra::{Result, RuntimeError};

/// Recovery state contains counters and references, never conversational content.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Checkpoint {
    pub step: u32,
    pub tool_index: u32,
    pub tool_calls: u32,
    pub output_tokens: u64,
    pub started_at_ms: u64,
    pub input_sequence: u64,
    pub request_event: Option<String>,
    pub response_event: Option<String>,
    pub tool_event: Option<String>,
    pub ending: Option<String>,
}

impl Checkpoint {
    pub fn turn(&self, request: &StartTurn) -> TurnContext {
        TurnContext {
            turn_id: request.turn_id.clone(),
            session_id: request.session_id.clone(),
            task_id: task_id(&request.turn_id),
            step: self.step,
        }
    }
}

pub(crate) fn task_id(turn: &str) -> String {
    stable(turn, "task")
}

pub(crate) fn stable(turn: &str, suffix: &str) -> String {
    format!(
        "agent.{}",
        hex::encode(Sha256::digest(format!("{turn}\0{suffix}").as_bytes()))
    )
}

pub(crate) fn now_ms() -> Result<u64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
        .ok_or_else(|| RuntimeError::invalid("agent_clock", "wall clock is outside its domain"))
}

pub(crate) fn invalid(detail: impl Into<String>) -> RuntimeError {
    RuntimeError::invalid("agent_state", detail)
}

pub(crate) fn encode<T: Serialize + ?Sized>(value: &T) -> Result<serde_json::Value> {
    serde_json::to_value(value).map_err(|error| invalid(error.to_string()))
}

pub(crate) fn decode<T: serde::de::DeserializeOwned>(value: serde_json::Value) -> Result<T> {
    serde_json::from_value(value).map_err(|error| invalid(error.to_string()))
}
