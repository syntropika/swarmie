use tembra::{core, prelude::*};

use crate::TurnContext;

/// Whether an interrupted operation may be invoked again with its original operation ID.
/// Safe means the implementation is read-only or durably deduplicates that identity.
/// Unsafe requires explicit reconciliation after an uncertain invocation; it is never a
/// promise that the first invocation had no effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", rename_all = "snake_case")]
#[schemars(crate = "tembra::schemars")]
pub enum ReplayPolicy {
    Safe,
    Unsafe,
}

/// Lifecycle-owned tool Registry metadata. Schema validation and mandatory authorization
/// apply to the final transformed arguments immediately before managed execution.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub arguments_schema: serde_json::Value,
    pub replay: ReplayPolicy,
}

/// Stable effect identity and model call identity are distinct. A replay retains both IDs.
/// Neither arguments nor hook contributions carry an authorization grant.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct ToolCall {
    pub turn: TurnContext,
    pub operation_id: String,
    pub tool_call_id: String,
    pub tool: String,
    pub arguments: serde_json::Value,
}

/// Confirmed tool outcome. An uncertain external effect must not be encoded as a successful
/// result. Ordinary tool errors can be shown to the model without losing their causal IDs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct ToolResult {
    pub operation_id: String,
    pub tool_call_id: String,
    pub output: serde_json::Value,
    pub is_error: bool,
}

/// Canonical tool operation schema. Applications serve their own named Contract with the
/// same method and contribute it to [`tool_definitions`]. This token is not a singleton
/// provider identity. The real terminal must independently enforce required permissions;
/// optional hook participation never grants authority.
#[tembra::contract(local, name = "swarmie.agent-tool-schema", version = 1)]
pub trait ToolHandler {
    #[portable]
    async fn execute(&self, call: ToolCall) -> Result<ToolResult>;
}

/// The actual Tembra Registry used by contributing tool Facets.
///
/// # Errors
/// Returns Tembra declaration errors; duplicate keys are rejected rather than shadowed.
pub fn tool_definitions() -> Result<core::Registry> {
    core::Registry::new("swarmie.agent-tools", core::ConflictPolicy::Error)
}

/// A veto is advice from a hook; mandatory authorization still runs at the real terminal.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct ToolDenial {
    pub reason: String,
}

/// Mandatory policy decision, evaluated after all argument transformations. No policy provider
/// is a refusal, never implicit permission. Tool implementations enforce their own authority too.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct ToolAuthorization {
    pub allowed: bool,
    pub reason: Option<String>,
}

/// Required final-boundary policy, independent from optional lifecycle contributions.
#[tembra::contract(local, name = "swarmie.agent-tool-policy", version = 1)]
pub trait ToolPolicy {
    #[portable]
    async fn authorize(&self, call: ToolCall) -> Result<ToolAuthorization>;
}
