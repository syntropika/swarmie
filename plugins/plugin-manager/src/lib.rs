//! Durable desired inventory and isolated installation of compiled Tembra orbs.
//!
//! Orb implementations do not depend on this crate. Their application composition
//! supplies a [`CorePlugin`] factory when plugin administration is wanted.

mod catalog;
mod index;
mod manager;
mod model;
mod orb;
mod storage;

pub use catalog::{Catalog, CatalogError, CorePlugin};
pub use index::{PluginContext, PluginIndex};
pub use model::{
    BlockReason, InstalledPlugin, MountFailure, OrbInfo, PluginId, PluginIdError, PluginSnapshot,
    PluginStatus, PluginView,
};
pub use orb::{ADMIN_PRINCIPAL, INSTANCE_KEY, ManagerMethod, SERVICE, orb};
pub use storage::{
    CommitOutcome, Inventory, PluginChange, PluginStorage, StorageError, StorageFactory,
};
