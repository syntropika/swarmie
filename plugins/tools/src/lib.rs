//! Tool metadata and provider routing. Tembra owns actual services and lifetimes.
//!
//! Use a Catalog directly or expose it with orb. Registration leases belong to
//! their declaring owner; no Agent Loop, task scheduler or storage is required.

mod annotations;
mod authoring;
mod catalog;
mod client;
#[cfg(all(feature = "coding", not(target_family = "wasm")))]
pub mod coding;
mod context;
mod error;
mod execution_error;
mod facts;
mod footprint;
pub mod hooks;
mod output;
mod policy;
mod provider;
mod routing;
mod schema;
mod typed;
pub mod validate;

pub use annotations::ToolAnnotations;
pub use authoring::{Tool, tool_spec};
pub use catalog::{Catalog, Registration, Selection, ToolDefinition};
pub use client::{CatalogMethod, CatalogService, INSTANCE_KEY, SERVICE, orb, orb_with_catalog};
pub use context::{ContextValue, ToolContext, ToolContextError, ToolResultContext};
pub use error::Reason;
pub(crate) use error::invalid;
pub use execution_error::{ToolErrorKind, ToolExecutionError};
pub use facts::{
    ChangeKind, ContentHash, ExitStatus, FileChange, FileRead, FileState, FilesChanged, FilesRead,
    Hunk, HunkLine, KeptEnd, Lines, LinesShown, OutputCut, content_hash,
};
pub use footprint::{Scope, Touches};
pub use output::{IntoToolOutput, ToolOutput};
pub use policy::ToolPolicy;
pub use provider::{
    ToolDecision, ToolExecution, ToolMethod, ToolPreparation, ToolProvider, ToolService, Tools,
};
pub use schema::args_schema;
pub use swarmie_providers::{
    AgentContext, OperationId, Recovery, ToolCall, ToolCallId, ToolName, ToolResult, ToolSpec,
};
pub use typed::{ToolHandler, tool_orb};
