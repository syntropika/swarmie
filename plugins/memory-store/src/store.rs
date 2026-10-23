use std::{cell::Cell, path::Path};

use rusqlite::{Connection, OptionalExtension, params};
use swarmie_sqlite_local::{Database, Schema};
use tembra::{RuntimeError, core::ErrorCode};

use crate::GetResult;

pub(crate) struct Store {
    database: Database,
    closed: Cell<bool>,
    owns_database: bool,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error("SQLite storage failed: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("shared SQLite storage failed: {0}")]
    Shared(#[from] swarmie_sqlite_local::Error),
    #[error("stored JSON is invalid: {0}")]
    Corrupt(#[from] serde_json::Error),
    #[error("the SQLite resource is closed")]
    Closed,
}

impl Error {
    pub(crate) fn into_runtime(self) -> RuntimeError {
        let reason = match &self {
            Self::Database(_) => "memory_store_storage",
            Self::Shared(swarmie_sqlite_local::Error::Closed) => "memory_store_closed",
            Self::Shared(_) => "memory_store_storage",
            Self::Corrupt(_) => "memory_store_corrupt",
            Self::Closed => "memory_store_closed",
        };
        RuntimeError::new(ErrorCode::Unavailable, reason, self.to_string())
    }
}

impl Store {
    pub(crate) fn open(path: &Path) -> Result<Self, Error> {
        let database = Database::open(
            &swarmie_sqlite_local::Configuration {
                database: path.into(),
            },
            vec![schema()],
        )?;
        Ok(Self {
            database,
            closed: Cell::new(false),
            owns_database: true,
        })
    }

    pub(crate) fn from_database(database: Database) -> Result<Self, Error> {
        database.with_connection(|connection| {
            connection.prepare("SELECT namespace,key,value FROM swarmie_memory_entries LIMIT 0")?;
            Ok::<_, Error>(())
        })?;
        Ok(Self {
            database,
            closed: Cell::new(false),
            owns_database: false,
        })
    }

    fn with<T>(&self, action: impl FnOnce(&Connection) -> Result<T, Error>) -> Result<T, Error> {
        if self.closed.get() {
            return Err(Error::Closed);
        }
        self.database.with_connection(action)
    }

    fn transaction<T>(
        &self,
        action: impl FnOnce(&Connection) -> Result<T, Error>,
    ) -> Result<T, Error> {
        if self.closed.get() {
            return Err(Error::Closed);
        }
        self.database.transaction(action)
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
        self.transaction(|connection| {
            connection.execute(
                "INSERT INTO swarmie_memory_entries (namespace, key, value) VALUES (?1, ?2, ?3)
                 ON CONFLICT(namespace, key) DO UPDATE SET value = excluded.value",
                params![namespace, key, encoded],
            )?;
            Ok(())
        })
    }

    pub(crate) fn delete(&self, namespace: &str, key: &str) -> Result<bool, Error> {
        self.transaction(|connection| {
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
        self.closed.set(true);
        if self.owns_database {
            self.database.close()?;
        }
        Ok(())
    }
}

pub(crate) fn schema() -> Schema {
    Schema {
        domain: "swarmie.memory-store",
        version: 1,
        initialize: |connection| {
            connection
                .execute_batch(
                    "CREATE TABLE IF NOT EXISTS swarmie_memory_entries (
                namespace TEXT NOT NULL, key TEXT NOT NULL, value TEXT NOT NULL,
                PRIMARY KEY(namespace,key)) WITHOUT ROWID;",
                )
                .map_err(|error| {
                    RuntimeError::unavailable("memory_store_storage", error.to_string())
                })
        },
        validate: |connection| {
            connection
                .prepare("SELECT namespace,key,value FROM swarmie_memory_entries LIMIT 0")
                .map(|_| ())
                .map_err(|error| {
                    RuntimeError::unavailable("memory_store_storage", error.to_string())
                })
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owning_connection_uses_normal_wal_after_open_and_reopen() -> Result<(), Error> {
        let directory = tempfile::tempdir().expect("test directory");
        let database = directory.path().join("store.sqlite");
        for _ in 0..2 {
            let store = Store::open(&database)?;
            store.with(|connection| {
                assert_eq!(
                    connection
                        .pragma_query_value(None, "synchronous", |row| row.get::<_, i64>(0))?,
                    1
                );
                assert_eq!(
                    connection
                        .pragma_query_value(None, "journal_mode", |row| row.get::<_, String>(0))?,
                    "wal"
                );
                Ok(())
            })?;
            store.close()?;
        }
        Ok(())
    }

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
    fn closing_one_shared_domain_preserves_the_physical_owner() -> Result<(), Error> {
        let directory = tempfile::tempdir().expect("test directory");
        let database = Database::open(
            &swarmie_sqlite_local::Configuration {
                database: directory.path().join("shared.sqlite"),
            },
            vec![schema()],
        )?;
        let store = Store::from_database(database.clone())?;
        store.put("notes", "committed", &serde_json::json!(1))?;
        store.close()?;
        assert!(matches!(
            store.get("notes", "committed"),
            Err(Error::Closed)
        ));
        let other = Store::from_database(database.clone())?;
        assert_eq!(
            other.get("notes", "committed")?,
            GetResult::Present {
                value: serde_json::json!(1)
            }
        );
        database.close()?;
        assert!(matches!(
            other.get("notes", "committed"),
            Err(Error::Shared(swarmie_sqlite_local::Error::Closed))
        ));
        Ok(())
    }
}
