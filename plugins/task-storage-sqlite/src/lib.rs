//! Durable transactional task storage provided by a lifecycle-owned SQLite Resource.
//!
//! The provider holds an operating-system ownership lock for its lifetime. A separate
//! scheduler claim excludes multiple schedulers sharing the same provider. Closure releases
//! both ownership boundaries without deleting committed tasks.

mod configuration;
mod migrations;
mod outbox;
mod service;
mod store;

pub use configuration::{Configuration, ConfigurationError, TaskOutboxLimits};
pub use service::{facet, facet_with_outbox_limits};

/// Logical identity of the compiled storage plugin.
pub const PLUGIN_ID: &str = "swarmie.task-storage-sqlite";

/// Executable identity of the SQLite storage provider.
pub const FACET_ID: &str = "swarmie.task-storage-sqlite.provider";

/// Version of the compiled plugin package.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
