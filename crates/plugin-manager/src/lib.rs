//! Durable local administration of actual Tembra plugins on one connected node.
//!
//! Ordinary capability calls use Tembra References and Registries directly. This crate owns
//! administrative intent and package bytes, while Tembra owns executable objects and reload.

mod activation;
mod administration;
mod browser;
mod browser_access;
mod cleanup;
mod composition;
mod core_plugins;
mod error;
mod lifecycle;
mod lifecycle_hooks;
mod manager;
mod observations;
mod policy;
mod replacement;
mod service;
mod source;
mod staging;
mod startup;
mod store;
mod validation;

pub use browser::MAX_PACKAGE_CHUNK_BYTES;
pub use composition::CompositionGuard;
pub use error::{ManagerError, Result};
pub use manager::PluginManager;
pub use source::LocalSource;
pub use startup::CorePluginDeployment;
