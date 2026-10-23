//! Independent durable task/session association and ordered idempotent history publication.
//!
//! Tembra owns References, managed activity and SQLite Resource lifetime. The task owner must
//! require a confirmed binding immediately before every external effect. Binding confirmation
//! does not replace cancellation, authorization or operation-specific uncertainty handling.

mod admission;
pub mod backend;
mod configuration;
mod mapping;
mod publisher;
mod service;
mod store;

pub use configuration::{Configuration, SharedConfiguration};
pub use service::{facet, facet_with_database};

/// Current binding schema for the shared physical owner. Initialization creates tables only;
/// the selected bridge later pins its task store, session store and consumer atomically.
pub fn sqlite_schema() -> swarmie_sqlite_local::Schema {
    swarmie_sqlite_local::Schema {
        domain: "task-session-bindings",
        version: 1,
        initialize: store::initialize_schema,
        validate: store::validate_schema,
    }
}

/// Compiled application plugin identity.
pub const PLUGIN_ID: &str = "swarmie.task-session-integration";

/// Stable managed caller identity owning the source consumer across Host replacement.
pub const FACET_ID: &str = "swarmie.task-session-integration.bridge";

/// Compiled plugin version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
