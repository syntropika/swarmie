//! Hook strategies are part of the versioned public descriptor. Series observers cannot
//! transform values; waterfalls can transform data but cannot replace causal identity.
//! Bail contributors may veto a tool call, while required authorization remains independent.
//! Handler failure stops a pipeline; priority ordering and physical settlement belong to Tembra.

use swarmie_model_contracts::{
    FinishReason, GenerateResult, Message, ModelRequest, SequencedEvent,
};
use swarmie_task_contracts::TaskOutcome;
use tembra::prelude::*;
use tembra_hooks::{HookDescriptor, HookType, RustHookDefinition, Strategy, TerminalProfile};

use crate::{ToolCall, ToolDenial, ToolResult, TurnContext};

/// Ephemeral context projection. A contribution can inject memory or compact messages without
/// rewriting the Session log. Only the actual model request is persisted as an explicit event.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct PreparedContext {
    pub turn: TurnContext,
    pub messages: Vec<Message>,
}

/// Typed model request extension point; the final model provider still validates capabilities
/// and request values. The loop rechecks request identity and turn resource limits.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct ModelCall {
    pub turn: TurnContext,
    pub request: ModelRequest,
}

/// A complete model outcome, including provider refusals. Streaming deltas are separate
/// ephemeral observations and do not cause a new durable task for every token.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct ModelObservation {
    pub call: ModelCall,
    pub result: GenerateResult,
}

/// Ephemeral ordered streaming observation. Sequence belongs to one physical generation;
/// deltas are not confirmed history and may be replayed or absent after recovery.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct ModelStreamObservation {
    pub turn: TurnContext,
    pub request_id: String,
    pub event: SequencedEvent,
}

/// Tool results are confirmed before notification. Observers cannot rewrite external outcomes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct ToolObservation {
    pub call: ToolCall,
    pub result: ToolResult,
}

/// Step summary follows complete processing of that step's model and tools.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct StepEnd {
    pub turn: TurnContext,
    pub finish_reason: FinishReason,
    pub tool_calls: u32,
}

/// Turn decision notification. The TaskRecord remains the source of truth for actual terminal
/// commitment and physical settlement. Contributions can observe repeated delivery on recovery.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct TurnEnd {
    pub turn: TurnContext,
    pub outcome: TaskOutcome,
}

fn portable<T: Serialize + JsonSchema + 'static>() -> Result<HookType<T>> {
    let schema = tembra::contracts::schema_for::<T>()
        .map_err(|error| RuntimeError::invalid("agent_hook_schema", error.to_string()))?;
    HookType::serde(schema)
}

fn series<A: Serialize + JsonSchema + 'static>(name: &str) -> Result<RustHookDefinition<A, ()>> {
    let input = portable::<A>()?;
    let descriptor = HookDescriptor::new(
        name,
        1,
        Strategy::Series,
        input.profile().clone(),
        None,
        None,
    )?;
    RustHookDefinition::from_descriptor(descriptor, input, HookType::absent())
}

fn waterfall<A: Serialize + JsonSchema + 'static>(name: &str) -> Result<RustHookDefinition<A, A>> {
    let input = portable::<A>()?;
    let descriptor = HookDescriptor::new(
        name,
        1,
        Strategy::Waterfall,
        input.profile().clone(),
        input.profile().clone(),
        None,
    )?;
    RustHookDefinition::from_descriptor(descriptor, input.clone(), input)
}

/// Ordered observers, including empty success. Repeated recovery delivery is possible.
///
/// # Errors
/// Returns Tembra schema or descriptor validation errors.
pub fn on_turn_start() -> Result<RustHookDefinition<TurnContext, ()>> {
    series("swarmie.agent.on-turn-start")
}

/// Ordered context transformations; an empty pipeline returns its original input.
///
/// # Errors
/// Returns Tembra schema or descriptor validation errors.
pub fn prepare_context() -> Result<RustHookDefinition<PreparedContext, PreparedContext>> {
    waterfall("swarmie.agent.prepare-context")
}

/// Ordered request transformations before intent publication and invocation.
///
/// # Errors
/// Returns Tembra schema or descriptor validation errors.
pub fn before_model_call() -> Result<RustHookDefinition<ModelCall, ModelCall>> {
    waterfall("swarmie.agent.before-model-call")
}

/// Ordered observers of confirmed model outcomes.
///
/// # Errors
/// Returns Tembra schema or descriptor validation errors.
pub fn after_model_call() -> Result<RustHookDefinition<ModelObservation, ()>> {
    series("swarmie.agent.after-model-call")
}

/// Ordered ephemeral stream observers. A failure stops consumption and cancels generation;
/// no durable task or Session event is created for every token. Sensitive reasoning deltas
/// must not be published into Session history by a generic observer.
///
/// # Errors
/// Returns Tembra schema or descriptor validation errors.
pub fn on_model_event() -> Result<RustHookDefinition<ModelStreamObservation, ()>> {
    series("swarmie.agent.on-model-event")
}

/// Ordered argument transformations. Final schema validation and mandatory authorization
/// must run after this waterfall and again at the actual effect boundary.
///
/// # Errors
/// Returns Tembra schema or descriptor validation errors.
pub fn before_tool_call() -> Result<RustHookDefinition<ToolCall, ToolCall>> {
    waterfall("swarmie.agent.before-tool-call")
}

/// First explicit Tembra `BailDecision::Stop(ToolDenial)` vetoes the invocation. An unresolved
/// hook means no optional veto; it never authorizes execution or bypasses ToolPolicy.
///
/// # Errors
/// Returns Tembra schema or descriptor validation errors.
pub fn authorize_tool_call() -> Result<RustHookDefinition<ToolCall, ToolDenial>> {
    let input = portable::<ToolCall>()?;
    let result = portable::<ToolDenial>()?;
    let descriptor = HookDescriptor::new(
        "swarmie.agent.authorize-tool-call",
        1,
        Strategy::Bail,
        input.profile().clone(),
        result.profile().clone(),
        None,
    )?;
    RustHookDefinition::from_descriptor(descriptor, input, result)
}

/// Ordered observers; tool outcomes remain unchanged.
///
/// # Errors
/// Returns Tembra schema or descriptor validation errors.
pub fn after_tool_call() -> Result<RustHookDefinition<ToolObservation, ()>> {
    series("swarmie.agent.after-tool-call")
}

/// Ordered complete-step observers.
///
/// # Errors
/// Returns Tembra schema or descriptor validation errors.
pub fn on_step_end() -> Result<RustHookDefinition<StepEnd, ()>> {
    series("swarmie.agent.on-step-end")
}

/// Ordered turn-decision observers; failures never roll back external effects.
///
/// # Errors
/// Returns Tembra schema or descriptor validation errors.
pub fn on_turn_end() -> Result<RustHookDefinition<TurnEnd, ()>> {
    series("swarmie.agent.on-turn-end")
}

/// Optional middleware around the actual managed model terminal, for tracing or measured
/// invocation. Tembra Next enforces continuation ownership and explicit recovery. A short
/// circuit is an explicit replacement outcome subject to the same response validation.
///
/// # Errors
/// Returns Tembra schema or descriptor validation errors.
pub fn wrap_model_call() -> Result<RustHookDefinition<ModelCall, GenerateResult>> {
    let input = portable::<ModelCall>()?;
    let result = portable::<GenerateResult>()?;
    let descriptor = HookDescriptor::new(
        "swarmie.agent.wrap-model-call",
        1,
        Strategy::Middleware,
        input.profile().clone(),
        result.profile().clone(),
        Some(TerminalProfile {
            input: input.profile().clone(),
            result: result.profile().clone(),
        }),
    )?;
    RustHookDefinition::from_descriptor(descriptor, input, result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tembra::ContractToken;

    #[test]
    fn every_descriptor_has_a_portable_versioned_profile() -> Result<()> {
        let definitions = [
            on_turn_start()?.descriptor().clone(),
            prepare_context()?.descriptor().clone(),
            before_model_call()?.descriptor().clone(),
            after_model_call()?.descriptor().clone(),
            on_model_event()?.descriptor().clone(),
            before_tool_call()?.descriptor().clone(),
            authorize_tool_call()?.descriptor().clone(),
            after_tool_call()?.descriptor().clone(),
            on_step_end()?.descriptor().clone(),
            on_turn_end()?.descriptor().clone(),
            wrap_model_call()?.descriptor().clone(),
        ];
        for definition in definitions {
            let encoded =
                serde_json::to_string(definition.descriptor()).expect("serializable descriptor");
            let decoded = HookDescriptor::from_json(&encoded)?;
            assert_eq!(decoded.descriptor().version, 1);
            assert_eq!(decoded.fingerprint(), definition.fingerprint());
            assert!(matches!(
                decoded.input(),
                Some(tembra::contracts::ValueSchema::Portable(_))
            ));
        }
        crate::AgentLoop.descriptor()?;
        crate::ToolHandler.descriptor()?;
        crate::ToolPolicy.descriptor()?;
        Ok(())
    }
}
