//! Tool metadata and provider routing. Tembra owns actual services and lifetimes.
//!
//! Use a Catalog directly or expose it with orb. Registration leases belong to
//! their declaring owner; no Agent Loop, task scheduler or storage is required.

mod catalog;
mod client;
mod error;
mod provider;
mod routing;

pub use catalog::{Catalog, Registration, Selection, ToolDefinition};
pub use client::{CatalogMethod, CatalogService, INSTANCE_KEY, SERVICE, orb};
pub use error::Reason;
pub(crate) use error::invalid;
pub use provider::{
    ToolDecision, ToolExecution, ToolMethod, ToolPreparation, ToolProvider, ToolService, Tools,
};
pub use swarmie_models::{
    AgentContext, OperationId, Recovery, ToolCall, ToolCallId, ToolName, ToolResult, ToolSpec,
};
