use std::{cell::RefCell, path::Path, time::Duration};

use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use tembra::{RuntimeError, core::ErrorCode};

use crate::GetResult;

pub(crate) struct Store {
    connection: RefCell<Option<Connection>>,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error("SQLite storage failed: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("stored JSON is invalid: {0}")]
    Corrupt(#[from] serde_json::Error),
    #[error("the SQLite resource is closed")]
    Closed,
}

impl Error {
    pub(crate) fn into_runtime(self) -> RuntimeError {
        let reason = match &self {
            Self::Database(_) => "memory_store_storage",
            Self::Corrupt(_) => "memory_store_corrupt",
            Self::Closed => "memory_store_closed",
        };
        RuntimeError::new(ErrorCode::Unavailable, reason, self.to_string())
    }
}

impl Store {
    pub(crate) fn open(path: &Path) -> Result<Self, Error> {
        let connection = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        // Refuse lock contention immediately rather than blocking the Host's local executor.
        connection.busy_timeout(Duration::ZERO)?;
        connection.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = FULL;
             CREATE TABLE IF NOT EXISTS swarmie_memory_entries (
                 namespace TEXT NOT NULL,
                 key TEXT NOT NULL,
                 value TEXT NOT NULL,
                 PRIMARY KEY (namespace, key)
             ) WITHOUT ROWID;",
        )?;
        Ok(Self {
            connection: RefCell::new(Some(connection)),
        })
    }

    fn with<T>(&self, action: impl FnOnce(&Connection) -> Result<T, Error>) -> Result<T, Error> {
        let connection = self.connection.borrow();
        action(connection.as_ref().ok_or(Error::Closed)?)
    }

    pub(crate) fn get(&self, namespace: &str, key: &str) -> Result<GetResult, Error> {
        self.with(|connection| {
            let value: Option<String> = connection
                .query_row(
                    "SELECT value FROM swarmie_memory_entries WHERE namespace = ?1 AND key = ?2",
                    params![namespace, key],
                    |row| row.get(0),
                )
                .optional()?;
            match value {
                None => Ok(GetResult::Missing),
                Some(value) => Ok(GetResult::Present {
                    value: serde_json::from_str(&value)?,
                }),
            }
        })
    }

    pub(crate) fn put(
        &self,
        namespace: &str,
        key: &str,
        value: &serde_json::Value,
    ) -> Result<(), Error> {
        let encoded = serde_json::to_string(value)?;
        self.with(|connection| {
            connection.execute(
                "INSERT INTO swarmie_memory_entries (namespace, key, value) VALUES (?1, ?2, ?3)
                 ON CONFLICT(namespace, key) DO UPDATE SET value = excluded.value",
                params![namespace, key, encoded],
            )?;
            Ok(())
        })
    }

    pub(crate) fn delete(&self, namespace: &str, key: &str) -> Result<bool, Error> {
        self.with(|connection| {
            Ok(connection.execute(
                "DELETE FROM swarmie_memory_entries WHERE namespace = ?1 AND key = ?2",
                params![namespace, key],
            )? > 0)
        })
    }

    pub(crate) fn list(&self, namespace: &str) -> Result<Vec<String>, Error> {
        self.with(|connection| {
            let mut statement = connection.prepare(
                "SELECT key FROM swarmie_memory_entries WHERE namespace = ?1 ORDER BY key COLLATE BINARY",
            )?;
            let rows = statement.query_map([namespace], |row| row.get(0))?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
    }

    pub(crate) fn close(&self) -> Result<(), Error> {
        if let Some(connection) = self.connection.borrow_mut().take() {
            connection.close().map_err(|(_, error)| error)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_revokes_retained_value_and_is_idempotent() -> Result<(), Error> {
        let path = tempfile::tempdir().expect("test directory");
        let store = Store::open(&path.path().join("memory.sqlite"))?;
        store.put("notes", "draft", &serde_json::json!(null))?;
        store.close()?;
        assert!(matches!(store.get("notes", "draft"), Err(Error::Closed)));
        store.close()?;
        let reopened = Store::open(&path.path().join("memory.sqlite"))?;
        assert_eq!(
            reopened.get("notes", "draft")?,
            GetResult::Present {
                value: serde_json::Value::Null
            }
        );
        Ok(())
    }

    #[test]
    fn dropping_the_resource_value_closes_connection_and_releases_its_write_lock()
    -> Result<(), Error> {
        let directory = tempfile::tempdir().expect("test directory");
        let database = directory.path().join("memory.sqlite");
        let store = Store::open(&database)?;
        store.put("notes", "committed", &serde_json::json!(1))?;
        store.with(|connection| {
            connection.execute_batch("BEGIN IMMEDIATE")?;
            Ok(())
        })?;
        let competitor = Connection::open(&database)?;
        competitor.busy_timeout(Duration::ZERO)?;
        assert!(competitor.execute_batch("BEGIN IMMEDIATE").is_err());
        drop(store);
        competitor.execute_batch("BEGIN IMMEDIATE; ROLLBACK")?;
        let reopened = Store::open(&database)?;
        assert_eq!(
            reopened.get("notes", "committed")?,
            GetResult::Present {
                value: serde_json::json!(1)
            }
        );
        Ok(())
    }
}
