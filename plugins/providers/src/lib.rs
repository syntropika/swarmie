//! Shared model requests, provider operations and native Tembra references.
//! This library has no Agent Loop, task scheduler, storage or concrete provider dependency.
mod progress;
mod provider;
pub mod settings;
mod types;

#[cfg(feature = "mock")]
pub mod mock;

pub use progress::{ModelProgress, Progress};
pub use provider::{Model, ModelMethod, ModelService, Recovery};
pub use types::{
    AgentContext, AssistantMessage, Message, ModelRequest, ModelResponse, OperationId, ToolCall,
    ToolCallId, ToolName, ToolResult, ToolSpec, Usage,
};

fn invalid(message: impl ToString) -> tembra::Error {
    tembra::Error::invalid("swarmie.models.input", message.to_string())
}
