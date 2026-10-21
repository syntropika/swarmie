use swarmie_task_contracts::TaskRecord;
use tembra::prelude::*;

/// Fixed upper bounds apply independently. A plugin may choose stricter operational bounds.
/// A zero bound is invalid. Time is elapsed wall time including waiting and recovery.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct TurnLimits {
    pub max_steps: u32,
    pub max_tool_calls: u32,
    pub max_duration_ms: u64,
    pub max_output_tokens: u64,
}

impl Default for TurnLimits {
    fn default() -> Self {
        Self {
            max_steps: 16,
            max_tool_calls: 32,
            max_duration_ms: 300_000,
            max_output_tokens: 32_768,
        }
    }
}

impl TurnLimits {
    /// Rejects unbounded or nonportable limits before submission or external effects.
    ///
    /// # Errors
    /// Returns `agent_limits` for zero bounds or integers outside the portable range.
    pub fn validate(&self) -> Result<()> {
        if self.max_steps == 0
            || self.max_tool_calls == 0
            || self.max_duration_ms == 0
            || self.max_output_tokens == 0
            || self.max_duration_ms > swarmie_task_contracts::MAX_PORTABLE_INTEGER
            || self.max_output_tokens > swarmie_task_contracts::MAX_PORTABLE_INTEGER
        {
            return Err(RuntimeError::invalid(
                "agent_limits",
                "turn limits must be positive portable integers",
            ));
        }
        Ok(())
    }
}

/// Stable causal identity available to observers and transforming contributions.
/// Identity fields cannot be rewritten by a transforming hook; the loop checks equality.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct TurnContext {
    pub turn_id: String,
    pub session_id: String,
    pub task_id: String,
    pub step: u32,
}

/// Caller-supplied identity makes a submission reconcilable after a lost response.
/// Input references an already committed Session message; history is never copied into
/// Durable Tasks checkpoints. Model selection is owned by Model Contract.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct StartTurn {
    pub turn_id: String,
    pub session_id: String,
    pub input_event_id: String,
    pub model: swarmie_model_contracts::ModelIdentity,
    pub limits: TurnLimits,
}

/// Exact turn identity, independent of the current plugin implementation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct TurnLookup {
    pub turn_id: String,
}

/// Committed task state is the authoritative execution observation. The Session separately
/// owns content and may be ahead of a task checkpoint after a cross-store interruption.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct TurnRecord {
    pub turn_id: String,
    pub session_id: String,
    pub task: TaskRecord,
}

/// Explicit evidence resolving an uncertain external operation. The implementation validates
/// that the referenced committed Session event belongs to the exact turn and operation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct KnownResult {
    pub operation_id: String,
    pub result_event_id: String,
}

/// Continue a recoverable turn, optionally supplying committed evidence of an external result.
/// There is no general permission to replay an unresolved unsafe operation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct ContinueTurn {
    pub turn_id: String,
    pub resolution: Option<KnownResult>,
}

/// Portable control surface shared by compatible agent-loop implementations.
/// Implementation selection uses ordinary Tembra Contract provider selection.
#[tembra::contract(local, name = "swarmie.agent-loop", version = 1)]
pub trait AgentLoop {
    #[portable]
    async fn start(&self, request: StartTurn) -> Result<TurnRecord>;

    #[portable]
    async fn inspect(&self, request: TurnLookup) -> Result<Option<TurnRecord>>;

    /// Records logical cancellation through Durable Tasks. A returned record does not prove
    /// physical settlement of already admitted external work.
    #[portable]
    async fn cancel(&self, request: TurnLookup) -> Result<TurnRecord>;

    /// Requests reconciliation of an interrupted or waiting turn. Unsafe unresolved effects
    /// remain blocked until their authoritative result is reconciled separately.
    #[portable]
    async fn continue_turn(&self, request: ContinueTurn) -> Result<TurnRecord>;
}
