//! Shared model requests, provider operations and native Tembra references.
//! This library has no Agent Loop, task scheduler, platform storage or concrete provider dependency.
pub mod accounts;
mod factory;
mod freshness;
mod prepared;
mod progress;
mod provider;
pub mod settings;
mod sources;
mod types;

#[cfg(feature = "mock")]
pub mod mock;

pub use accounts::recovery::ProtocolBinding;
pub use factory::{
    AccountRecord, CapturedModel, CredentialRequirement, EmbeddedCatalogBinding, ModelConstruction,
    ModelFactory, ModelMetadataInput, ModelSelection,
};
pub use freshness::{MetadataFreshness, MetadataTimestamp};
pub use prepared::{
    Component, ComponentKind, CountPrecision, Measurement, OperationalModel, OutputReservation,
    PreparationPolicy, PreparedDescription, PreparedModelRequest,
};
pub use progress::{ModelProgress, Progress};
pub use provider::{Model, ModelMethod, ModelService, Recovery};
pub use types::{
    AgentContext, AssistantMessage, Message, ModelRequest, ModelResponse, OperationId,
    SubmissionProtection, ToolCall, ToolCallId, ToolName, ToolResult, ToolSpec, Usage,
};

fn invalid(message: impl ToString) -> tembra::Error {
    tembra::Error::invalid("swarmie.models.input", message.to_string())
}

pub use sources::{
    ContextSources, MAX_CONTEXT_SOURCE_BYTES, MAX_SOURCE_BYTES, SourceKind, SourcePart,
    SourceScope, digest as source_digest,
};
