use crate::{InstalledPlugin, PluginId};
use futures::future::LocalBoxFuture;
use serde::{Deserialize, Serialize};
use std::rc::Rc;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Inventory {
    pub revision: u64,
    pub plugins: Vec<InstalledPlugin>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum PluginChange {
    Put(InstalledPlugin),
    Remove(PluginId),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CommitOutcome {
    Applied { revision: u64 },
    Rejected { current_revision: u64 },
    Unknown { message: String },
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum StorageError {
    #[error(transparent)]
    Owner(#[from] swarmie_storage::OwnerError),
    #[error("storage is closed")]
    Closed,
    #[error("storage is unavailable: {0}")]
    Unavailable(String),
    #[error("stored plugin inventory is invalid: {0}")]
    InvalidInventory(String),
}

/// Administrative inventory capability supplied by the host's storage resource.
///
/// This is not a universal data model or a multidomain transaction protocol.
/// Implementations may use any backend satisfying these atomic guarantees.
pub trait PluginStorage {
    /// Shared authority identity and uncertainty gate of the backing owner.
    fn owner(&self) -> swarmie_storage::StorageOwner;

    /// Reads records and their revision from one consistent snapshot.
    fn inventory(&self) -> LocalBoxFuture<'_, Result<Inventory, StorageError>>;

    /// Applies the whole batch only if `expected_revision` still matches.
    ///
    /// Rejection and errors returned before submission leave every record unchanged.
    /// Uncertain settlement must return [`CommitOutcome::Unknown`], never rejection.
    /// An empty batch validates the revision without advancing it.
    fn commit(
        &self,
        expected_revision: u64,
        changes: Vec<PluginChange>,
    ) -> LocalBoxFuture<'_, Result<CommitOutcome, StorageError>>;

    /// Closes owned storage after users have physically settled.
    fn close(&self) -> LocalBoxFuture<'_, Result<(), StorageError>>;
}

/// Opens storage during Tembra-managed resource activation, never in a orb factory.
pub type StorageFactory =
    Rc<dyn Fn() -> LocalBoxFuture<'static, Result<Rc<dyn PluginStorage>, StorageError>>>;
