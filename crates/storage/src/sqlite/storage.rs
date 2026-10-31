use crate::sqlite::{SqliteOwner, owner::Lease};
use futures::{FutureExt, future::LocalBoxFuture};
use rusqlite::{Connection, TransactionBehavior, params};
use std::{
    num::NonZeroU32,
    path::{Path, PathBuf},
};
use swarmie_plugin_manager::{
    CommitOutcome, InstalledPlugin, Inventory, PluginChange, PluginId, PluginStorage, StorageError,
    StorageFactory,
};

/// A file-backed storage owner with atomic, revision-checked inventory commits.
pub struct SqliteStorage {
    pub(crate) lease: Lease,
}

impl SqliteStorage {
    /// Opens a standalone inventory capability. Use SqliteOwner to share domains.
    /// Unsupported schemas are refused without deleting data.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        SqliteOwner::new(path.as_ref().to_path_buf()).inventory()
    }
}

/// Creates a lazy inventory factory backed by one owner incarnation.
pub fn factory(path: impl Into<PathBuf>) -> StorageFactory {
    SqliteOwner::new(path).inventory_factory()
}

impl PluginStorage for SqliteStorage {
    fn owner(&self) -> swarmie_storage_core::StorageOwner {
        self.lease.database.health.clone()
    }

    fn inventory(&self) -> LocalBoxFuture<'_, Result<Inventory, StorageError>> {
        async move {
            self.lease.check()?;
            let mut stored = self.lease.database.connection.borrow_mut();
            let connection = stored.as_mut().ok_or(StorageError::Closed)?;
            let transaction = connection.transaction().map_err(sql_error)?;
            let revision = revision(&transaction)?;
            let mut statement = transaction
                .prepare("SELECT id,version,enabled FROM swarmie_plugins ORDER BY id")
                .map_err(sql_error)?;
            let rows = statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, u32>(1)?,
                        row.get::<_, bool>(2)?,
                    ))
                })
                .map_err(sql_error)?;
            let mut plugins = Vec::new();
            for row in rows {
                let (id, version, enabled) = row.map_err(sql_error)?;
                plugins.push(InstalledPlugin {
                    id: PluginId::new(id)
                        .map_err(|error| StorageError::InvalidInventory(error.to_string()))?,
                    version: NonZeroU32::new(version).ok_or_else(|| {
                        StorageError::InvalidInventory("zero plugin version".into())
                    })?,
                    enabled,
                });
            }
            drop(statement);
            transaction.commit().map_err(sql_error)?;
            Ok(Inventory { revision, plugins })
        }
        .boxed_local()
    }

    fn commit(
        &self,
        expected_revision: u64,
        changes: Vec<PluginChange>,
    ) -> LocalBoxFuture<'_, Result<CommitOutcome, StorageError>> {
        async move {
            self.lease.check()?;
            self.owner().check()?;
            let mut stored = self.lease.database.connection.borrow_mut();
            let connection = stored.as_mut().ok_or(StorageError::Closed)?;
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(sql_error)?;
            let current_revision = revision(&transaction)?;
            if current_revision != expected_revision {
                return Ok(CommitOutcome::Rejected { current_revision });
            }
            if changes.is_empty() {
                return Ok(CommitOutcome::Applied {
                    revision: current_revision,
                });
            }
            let next = current_revision
                .checked_add(1)
                .and_then(|value| i64::try_from(value).ok())
                .ok_or_else(|| StorageError::InvalidInventory("inventory revision exhausted".into()))?;
            for change in changes {
                match change {
                    PluginChange::Put(record) => {
                        transaction
                            .execute(
                                "INSERT INTO swarmie_plugins(id,version,enabled) VALUES(?1,?2,?3)
                                ON CONFLICT(id) DO UPDATE SET version=excluded.version,enabled=excluded.enabled",
                                params![record.id.as_str(), record.version.get(), record.enabled],
                            )
                            .map_err(sql_error)?;
                    }
                    PluginChange::Remove(id) => {
                        transaction
                            .execute("DELETE FROM swarmie_plugins WHERE id=?1", [id.as_str()])
                            .map_err(sql_error)?;
                    }
                }
            }
            transaction
                .execute(
                    "UPDATE swarmie_plugins_meta SET revision=?1 WHERE singleton=1",
                    [next],
                )
                .map_err(sql_error)?;
            match transaction.commit() {
                Ok(()) => Ok(CommitOutcome::Applied { revision: next as u64 }),
                Err(error) => {
                    drop(stored);
                    self.owner().require_recovery();
                    Ok(CommitOutcome::Unknown {
                        message: error.to_string(),
                    })
                }
            }
        }
        .boxed_local()
    }

    fn close(&self) -> LocalBoxFuture<'_, Result<(), StorageError>> {
        async move { self.lease.close().map_err(StorageError::Unavailable) }.boxed_local()
    }
}

fn revision(connection: &Connection) -> Result<u64, StorageError> {
    connection
        .query_row(
            "SELECT revision FROM swarmie_plugins_meta WHERE singleton=1",
            [],
            |row| row.get(0),
        )
        .map_err(sql_error)
}

fn sql_error(error: rusqlite::Error) -> StorageError {
    if crate::sqlite::owner::invalid_sql(&error) {
        StorageError::InvalidInventory(error.to_string())
    } else {
        StorageError::Unavailable(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_owning_connection_establishes_wal_normal() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("runtime.sqlite");
        for _ in 0..2 {
            let owner = SqliteStorage::open(&file).unwrap();
            {
                let stored = owner.lease.database.connection.borrow();
                let connection = stored.as_ref().unwrap();
                assert_eq!(
                    connection
                        .query_row("PRAGMA journal_mode", [], |row| row.get::<_, String>(0))
                        .unwrap(),
                    "wal"
                );
                assert_eq!(
                    connection
                        .query_row("PRAGMA synchronous", [], |row| row.get::<_, u32>(0))
                        .unwrap(),
                    1
                );
            }
            futures::executor::block_on(owner.close()).unwrap();
        }
    }
}
