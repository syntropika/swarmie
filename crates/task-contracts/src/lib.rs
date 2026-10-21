//! Portable durable task records and application Contracts, independent of any storage backend.
//!
//! A checkpoint describes the next resumable phase. Committing an intention before an effect
//! does not make that effect exactly once: interrupted phases may execute again with the same
//! operation identity. Only a successful TaskStorage commit establishes durable progress.

#[cfg(feature = "conformance")]
pub mod conformance;

mod lifecycle;
mod model;
mod outbox;
mod schema;
mod storage;
mod tasks;
mod transport;

pub use lifecycle::*;
pub use model::*;
pub use outbox::*;
pub use storage::*;
pub use tasks::*;
pub use transport::*;
