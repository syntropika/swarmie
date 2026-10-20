//! Durable, authorized administration of actual Tembra plugins on one connected node.
//!
//! Ordinary capability calls use Tembra References and Registries directly. This crate owns
//! administrative intent and package bytes, while Tembra owns executable objects and reload.

mod administration;
mod browser;
mod error;
mod lifecycle;
mod manager;
mod service;
mod source;
mod store;
mod validation;

pub use browser::MAX_PACKAGE_CHUNK_BYTES;
pub use error::{ManagerError, Result};
pub use manager::PluginManager;
pub use source::LocalSource;
