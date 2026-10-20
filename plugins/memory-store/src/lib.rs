//! A compiled, namespaced JSON key-value plugin backed by a lifecycle-owned SQLite resource.
//!
//! This stores application data without interpreting its meaning. The database file survives
//! Host shutdown. Native declarations are replaced by starting a fresh Host, not by hot reload.

mod configuration;
mod contract;
mod service;
mod store;

pub use configuration::{Configuration, ConfigurationError};
pub use contract::{
    DeleteRequest, GetRequest, GetResult, ListRequest, MemoryStore, MemoryStoreInput,
    MemoryStoreProvider, MemoryStoreRef, PutRequest,
};
pub use service::facet;

/// Logical identity of the compiled plugin package.
pub const PLUGIN_ID: &str = "swarmie.memory-store";

/// Executable identity, independent of the logical package identity.
pub const FACET_ID: &str = "swarmie.memory-store.sqlite";

/// Version of this compiled plugin package.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
