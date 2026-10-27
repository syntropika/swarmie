//! One agent engine for ordinary invocations and optional recoverable tasks.
//!
//! Model and tool providers own external operations and their reconciliation. The
//! engine confirms intent before dispatch and results before advancing. It owns
//! neither a conversation database nor an alternative extension registry.

mod engine;
mod failure;
pub mod hooks;
mod model;
mod orb;
mod providers;

#[cfg(feature = "durable-tasks")]
pub mod durable;

#[cfg(feature = "conversations")]
pub mod conversation;

pub use engine::Agent;
pub use failure::{ExecutionFailure, ExecutionResult};
pub use model::{
    AgentContext, AssistantMessage, FailureReason, FinishReason, Message, ModelRequest,
    ModelResponse, OperationId, Output, Phase, Request, State, ToolCall, ToolCallId, ToolName,
    ToolResult, ToolSpec, Usage,
};
pub use orb::{Method, SERVICE, orb};
pub use providers::{
    Model, ModelMethod, ModelService, Recovery, ToolDecision, ToolExecution, ToolMethod,
    ToolPreparation, ToolProvider, ToolService, Tools,
};
