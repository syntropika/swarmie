use crate::sqlite::{SqliteOwner, owner::Lease};
use futures::{FutureExt, future::LocalBoxFuture};
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use std::path::Path;
use swarmie_activity::{Snapshot, Storage, StorageError};

/// A principal-scoped snapshot lease. This capability does not delete foreign records.
pub struct SqliteActivityStorage {
    pub(crate) lease: Lease,
}

impl SqliteActivityStorage {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        SqliteOwner::new(path.as_ref().to_path_buf()).activity()
    }

    fn check(&self) -> Result<(), StorageError> {
        self.lease.check()?;
        self.lease.database.health.check()?;
        Ok(())
    }
}

impl Storage for SqliteActivityStorage {
    fn load<'a>(
        &'a self,
        principal: &'a str,
    ) -> LocalBoxFuture<'a, Result<Snapshot, StorageError>> {
        async move {
            self.check()?;
            let stored = self.lease.database.connection.borrow();
            let connection = stored.as_ref().ok_or(StorageError::Closed)?;
            let raw: Option<(u64, String)> = connection
                .query_row(
                    "SELECT revision,data FROM swarmie_activity WHERE principal=?1",
                    [principal],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()
                .map_err(sql_error)?;
            let Some((revision, raw)) = raw else {
                return Ok(Snapshot::default());
            };
            let value: Snapshot = serde_json::from_str(&raw)
                .map_err(|error| StorageError::Invalid(error.to_string()))?;
            if revision != value.revision {
                return Err(StorageError::Invalid("snapshot revision differs".into()));
            }
            value
                .validate()
                .map_err(|error| StorageError::Invalid(error.to_string()))?;
            Ok(value)
        }
        .boxed_local()
    }

    fn replace<'a>(
        &'a self,
        principal: &'a str,
        expected: u64,
        next: Snapshot,
    ) -> LocalBoxFuture<'a, Result<(), StorageError>> {
        async move {
            self.check()?;
            next.validate().map_err(|error| StorageError::Invalid(error.to_string()))?;
            if expected.checked_add(1) != Some(next.revision) { return Err(StorageError::Invalid("replacement revision must advance once".into())); }
            let revision = i64::try_from(next.revision).map_err(|_| StorageError::Invalid("revision exhausted".into()))?;
            let data = serde_json::to_string(&next).map_err(|error| StorageError::Invalid(error.to_string()))?;
            let mut stored = self.lease.database.connection.borrow_mut();
            let connection = stored.as_mut().ok_or(StorageError::Closed)?;
            let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sql_error)?;
            let current: Option<u64> = transaction.query_row(
                "SELECT revision FROM swarmie_activity WHERE principal=?1", [principal], |row| row.get(0)
            ).optional().map_err(sql_error)?;
            if current.unwrap_or(0) != expected { return Err(StorageError::Conflict); }
            transaction.execute(
                "INSERT INTO swarmie_activity(principal,revision,data) VALUES(?1,?2,?3) ON CONFLICT(principal) DO UPDATE SET revision=excluded.revision,data=excluded.data",
                params![principal,revision,data],
            ).map_err(sql_error)?;
            if let Err(error) = transaction.commit() {
                drop(stored);
                self.lease.database.health.require_recovery();
                return Err(StorageError::Unknown(error.to_string()));
            }
            Ok(())
        }.boxed_local()
    }

    fn close(&self) -> LocalBoxFuture<'_, Result<(), StorageError>> {
        async move { self.lease.close().map_err(StorageError::Unavailable) }.boxed_local()
    }
}

fn sql_error(error: rusqlite::Error) -> StorageError {
    if crate::sqlite::owner::invalid_sql(&error) {
        StorageError::Invalid(error.to_string())
    } else {
        StorageError::Unavailable(error.to_string())
    }
}
