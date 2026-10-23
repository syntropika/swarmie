//! Durable transactional task storage provided by a lifecycle-owned SQLite Resource.
//!
//! The provider holds an operating-system ownership lock for its lifetime. A separate
//! scheduler claim excludes multiple schedulers sharing the same provider. Closure releases
//! both ownership boundaries without deleting committed tasks.

mod configuration;
mod outbox;
mod schema;
mod service;
mod store;

pub use configuration::{Configuration, ConfigurationError};
pub use service::facet;
pub use swarmie_task_contracts::TaskOutboxLimits;

/// Logical identity of the compiled storage plugin.
pub const PLUGIN_ID: &str = "swarmie.task-storage-sqlite";

/// Executable identity of the SQLite storage provider.
pub const FACET_ID: &str = "swarmie.task-storage-sqlite.provider";

/// Version of the compiled plugin package.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Native shared-owner adapters and explicit transaction operations.
pub mod backend;
pub use backend::{TaskStoreHandle, facet_with_database, sqlite_schema};
