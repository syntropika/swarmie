//! Independent durable task/session association and ordered idempotent history publication.
//!
//! Tembra owns References, managed activity and SQLite Resource lifetime. The task owner must
//! require a confirmed binding immediately before every external effect. Binding confirmation
//! does not replace cancellation, authorization or operation-specific uncertainty handling.

mod admission;
mod configuration;
mod mapping;
mod publisher;
mod service;
mod store;

pub use configuration::Configuration;
pub use service::facet;

/// Compiled application plugin identity.
pub const PLUGIN_ID: &str = "swarmie.task-session-integration";

/// Stable managed caller identity owning the source consumer across Host replacement.
pub const FACET_ID: &str = "swarmie.task-session-integration.bridge";

/// Compiled plugin version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
