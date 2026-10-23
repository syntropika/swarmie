use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use swarmie_agent_contracts::{ContinueTurn, StartTurn, TurnContext, TurnLookup};
use tembra::{Result, RuntimeError};

/// Recovery state contains counters and references, never conversational content.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Checkpoint {
    pub version: u32,
    pub session_id: String,
    pub conversation_id: String,
    pub turn_id: String,
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
    pub fn validate_identity(&self, request: &StartTurn) -> Result<()> {
        if self.version != 2
            || self.session_id != request.session_id
            || self.conversation_id != request.conversation_id
            || self.turn_id != request.turn_id
        {
            return Err(invalid(
                "checkpoint version or persisted conversation identity differs",
            ));
        }
        Ok(())
    }

    pub fn turn(&self, _request: &StartTurn) -> TurnContext {
        TurnContext {
            turn_id: self.turn_id.clone(),
            session_id: self.session_id.clone(),
            conversation_id: self.conversation_id.clone(),
            task_id: task_id(self),
            step: self.step,
        }
    }
}

/// Internal causal scope, never a second public session or task identity contract.
pub(crate) trait TurnIdentity {
    fn session_id(&self) -> &str;
    fn conversation_id(&self) -> &str;
    fn turn_id(&self) -> &str;
}

macro_rules! identity {
    ($($kind:ty),+ $(,)?) => {$ (
        impl TurnIdentity for $kind {
            fn session_id(&self) -> &str { &self.session_id }
            fn conversation_id(&self) -> &str { &self.conversation_id }
            fn turn_id(&self) -> &str { &self.turn_id }
        }
    )+ };
}

identity!(StartTurn, TurnContext, TurnLookup, ContinueTurn, Checkpoint);

pub(crate) fn task_id(turn: &impl TurnIdentity) -> String {
    stable(turn, "task")
}

/// Turn scope whose task is derived from the turn identity, never taken from a caller's value.
pub(crate) fn turn_scope(turn: &impl TurnIdentity) -> TurnContext {
    TurnContext {
        session_id: turn.session_id().into(),
        conversation_id: turn.conversation_id().into(),
        turn_id: turn.turn_id().into(),
        task_id: task_id(turn),
        step: 0,
    }
}

pub(crate) fn stable(turn: &impl TurnIdentity, suffix: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(b"swarmie.agent.v2");
    for part in [
        turn.session_id(),
        turn.conversation_id(),
        turn.turn_id(),
        suffix,
    ] {
        hash.update((part.len() as u64).to_be_bytes());
        hash.update(part.as_bytes());
    }
    format!("agent.{}", hex::encode(hash.finalize()))
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
