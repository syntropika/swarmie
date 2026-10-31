//! Shared storage API with a default SQLite backend.
//!
//! Disable default features to use the backend-neutral contracts alone.

pub use swarmie_storage_core::*;

#[cfg(feature = "sqlite")]
pub mod sqlite;
