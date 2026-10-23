use std::{cell::Cell, path::Path};

use rusqlite::{Connection, OptionalExtension, params};
use swarmie_plugin_contracts::*;
use swarmie_sqlite_local::{Database, Schema};

use crate::{ManagerError, Result};

mod settlement;

/// Administrative domain access over the shared physical SQLite owner.
pub(crate) struct Store {
    database: Database,
    closed: Cell<bool>,
    owns_database: bool,
}

#[derive(Clone)]
pub(crate) struct CachedPlugin {
    pub package: AcquiredPackage,
    pub desired: DesiredState,
    pub observed: ObservedState,
    pub error: String,
    pub activation_id: String,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
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

    pub fn from_database(database: Database) -> Result<Self> {
        database.with_connection(|connection| {
            connection.prepare("SELECT singleton,declared FROM composition LIMIT 0")?;
            Ok::<_, ManagerError>(())
        })?;
        Ok(Self {
            database,
            closed: Cell::new(false),
            owns_database: false,
        })
    }

    pub fn composition(&self) -> Result<Option<serde_json::Value>> {
        self.with(|connection| {
            let declared: Option<String> = connection
                .query_row(
                    "SELECT declared FROM composition WHERE singleton=1",
                    [],
                    |row| row.get(0),
                )
                .optional()?;
            declared
                .map(|value| serde_json::from_str(&value).map_err(ManagerError::from))
                .transpose()
        })
    }

    pub fn save_composition(&self, composition: &serde_json::Value) -> Result<()> {
        self.transaction(|connection| {
            connection.execute("INSERT INTO composition(singleton,declared) VALUES(1,?1) ON CONFLICT(singleton) DO UPDATE SET declared=excluded.declared", [serde_json::to_string(composition)?])?;
            Ok(())
        })
    }

    pub fn close(&self) -> Result<()> {
        self.closed.set(true);
        if self.owns_database {
            self.database.close()?;
        }
        Ok(())
    }

    fn with<T>(&self, action: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        if self.closed.get() {
            return Err(ManagerError::Closed);
        }
        self.database.with_connection(action)
    }

    fn transaction<T>(&self, action: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        if self.closed.get() {
            return Err(ManagerError::Closed);
        }
        self.database.transaction(action)
    }

    pub fn cached_version(&self, id: &str, version: &str) -> Result<Option<AcquiredPackage>> {
        self.with(|connection| {
            let bytes: Option<Vec<u8>> = connection
                .query_row(
                    "SELECT package FROM plugin_versions WHERE plugin_id=?1 AND version=?2",
                    params![id, version],
                    |row| row.get(0),
                )
                .optional()?;
            bytes
                .map(|bytes| serde_json::from_slice(&bytes).map_err(ManagerError::from))
                .transpose()
        })
    }

    /// Retains exact approved bytes without changing the installed version selection.
    pub fn retain_version(&self, package: &AcquiredPackage) -> Result<()> {
        self.transaction(|connection| {
            let bytes = serde_json::to_vec(package)?;
            let existing: Option<Vec<u8>> = connection.query_row(
                "SELECT package FROM plugin_versions WHERE plugin_id=?1 AND version=?2",
                params![package.manifest.id, package.manifest.version], |row| row.get(0),
            ).optional()?;
            if existing.as_ref().is_some_and(|existing| existing != &bytes) {
                return Err(ManagerError::Integrity("an approved version cannot change its cached bytes".into()));
            }
            connection.execute("INSERT OR IGNORE INTO plugin_versions(plugin_id,version,package) VALUES (?1,?2,?3)",
                params![package.manifest.id, package.manifest.version, bytes])?;
            Ok(())
        })
    }

    /// Retains verified bytes, then updates the selected version transactionally.
    pub fn save(&self, plugin: &CachedPlugin) -> Result<()> {
        self.retain_version(&plugin.package)?;
        self.transaction(|connection| {
            let transaction = connection;
            let package = &plugin.package;
            transaction.execute(
                "INSERT INTO installed_plugins(plugin_id,version,desired,observed,error,activation_id)
                 VALUES (?1,?2,?3,?4,?5,?6) ON CONFLICT(plugin_id) DO UPDATE SET
                 version=excluded.version, desired=excluded.desired,
                 observed=excluded.observed, error=excluded.error, activation_id=excluded.activation_id",
                params![
                    package.manifest.id,
                    package.manifest.version,
                    serde_json::to_string(&plugin.desired)?,
                    serde_json::to_string(&plugin.observed)?,
                    plugin.error,
                    plugin.activation_id
                ],
            )?;
            Ok(())
        })
    }

    pub fn load(&self, id: &str) -> Result<Option<CachedPlugin>> {
        self.with(|connection| {
            let row = connection
                .query_row(
                    "SELECT v.package,p.desired,p.observed,p.error,p.activation_id FROM installed_plugins p
                 JOIN plugin_versions v ON v.plugin_id=p.plugin_id AND v.version=p.version
                 WHERE p.plugin_id=?1",
                    [id],
                    |row| {
                        Ok((
                            row.get::<_, Vec<u8>>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, String>(4)?,
                        ))
                    },
                )
                .optional()?;
            row.map(|(bytes, desired, observed, error, activation_id)| {
                Ok(CachedPlugin {
                    package: serde_json::from_slice(&bytes)?,
                    desired: serde_json::from_str(&desired)?,
                    observed: serde_json::from_str(&observed)?,
                    error,
                    activation_id,
                })
            })
            .transpose()
        })
    }

    pub fn all(&self) -> Result<Vec<CachedPlugin>> {
        let ids = self.with(|connection| {
            let mut statement =
                connection.prepare("SELECT plugin_id FROM installed_plugins ORDER BY plugin_id")?;
            let ids = statement
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            Ok(ids)
        })?;
        ids.iter()
            .map(|id| {
                self.load(id)?
                    .ok_or_else(|| ManagerError::NotInstalled(id.clone()))
            })
            .collect()
    }

    pub fn remove(&self, id: &str) -> Result<()> {
        self.transaction(|connection| {
            connection.execute("DELETE FROM installed_plugins WHERE plugin_id=?1", [id])?;
            Ok(())
        })
    }

    pub fn browser_sessions(&self) -> Result<Vec<BrowserSessionRecord>> {
        self.with(|connection| {
            let mut statement =
                connection.prepare("SELECT record FROM browser_sessions ORDER BY rowid")?;
            let records = statement
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            records
                .into_iter()
                .map(|record| Ok(serde_json::from_str(&record)?))
                .collect()
        })
    }

    pub fn browser_session(&self, record: &BrowserSessionRecord) -> Result<()> {
        self.transaction(|connection| {
            connection.execute(
                "INSERT INTO browser_sessions(plugin_id,session_id,activation_id,record) VALUES (?1,?2,?3,?4)
                ON CONFLICT(plugin_id,session_id,activation_id) DO UPDATE SET record=excluded.record",
                params![
                    record.plugin_id,
                    record.session_id,
                    record.activation_id,
                    serde_json::to_string(record)?
                ],
            )?;
            Ok(())
        })
    }

    pub fn browser_removal_requested(&self, id: &str) -> Result<()> {
        for mut record in self
            .browser_sessions()?
            .into_iter()
            .filter(|record| record.plugin_id == id)
        {
            if record.observed != BrowserObservation::Inactive {
                let pending = "browser removal requested; awaiting browser acknowledgement";
                if !record.error.contains(pending) {
                    record.error = if record.error.is_empty() {
                        pending.into()
                    } else {
                        format!("{}; {pending}", record.error)
                    };
                }
                self.browser_session(&record)?;
            }
        }
        Ok(())
    }

    pub fn configure(&self, id: &str, configuration: &serde_json::Value) -> Result<()> {
        self.transaction(|connection| {
            connection.execute(
                "INSERT INTO provider_configuration(provider_id,configuration) VALUES (?1,?2)
                ON CONFLICT(provider_id) DO UPDATE SET configuration=excluded.configuration",
                params![id, serde_json::to_string(configuration)?],
            )?;
            Ok(())
        })
    }

    pub fn configurations(&self) -> Result<Vec<(String, serde_json::Value)>> {
        self.with(|connection| {
            let mut statement = connection.prepare(
                "SELECT provider_id,configuration FROM provider_configuration ORDER BY provider_id",
            )?;
            let rows = statement
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            rows.into_iter()
                .map(|(id, text)| Ok((id, serde_json::from_str(&text)?)))
                .collect()
        })
    }

    pub fn operation(&self, operation: &OperationRecord) -> Result<()> {
        self.transaction(|connection| {
            connection.execute("INSERT INTO operations(id,plugin_id,operation,status,error) VALUES (?1,?2,?3,?4,?5)
                ON CONFLICT(id) DO UPDATE SET status=excluded.status,error=excluded.error",
                params![operation.id,operation.plugin_id,serde_json::to_string(&operation.operation)?,
                    serde_json::to_string(&operation.status)?,operation.error])?;
            Ok(())
        })
    }

    pub fn operations(&self) -> Result<Vec<OperationRecord>> {
        self.with(|connection| {
            let mut statement = connection.prepare(
                "SELECT id,plugin_id,operation,status,error FROM operations ORDER BY rowid",
            )?;
            let rows = statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                    ))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            rows.into_iter()
                .map(|(id, plugin_id, operation, status, error)| {
                    Ok(OperationRecord {
                        id,
                        plugin_id,
                        operation: serde_json::from_str(&operation)?,
                        status: serde_json::from_str(&status)?,
                        error,
                    })
                })
                .collect()
        })
    }
}

pub(crate) fn schema() -> Schema {
    Schema {
        domain: "swarmie.plugin-administration",
        version: 1,
        initialize: |connection| {
            connection.execute_batch("CREATE TABLE IF NOT EXISTS composition (
                 singleton INTEGER PRIMARY KEY CHECK(singleton=1), declared TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS plugin_versions (
                 plugin_id TEXT NOT NULL, version TEXT NOT NULL, package BLOB NOT NULL,
                 PRIMARY KEY(plugin_id, version));
             CREATE TABLE IF NOT EXISTS installed_plugins (
                 plugin_id TEXT PRIMARY KEY, version TEXT NOT NULL,
                 desired TEXT NOT NULL, observed TEXT NOT NULL, error TEXT NOT NULL DEFAULT '',
                 activation_id TEXT NOT NULL,
                 FOREIGN KEY(plugin_id,version) REFERENCES plugin_versions(plugin_id,version));
             CREATE TABLE IF NOT EXISTS provider_configuration (
                 provider_id TEXT PRIMARY KEY, configuration TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS browser_sessions (
                 plugin_id TEXT NOT NULL, session_id TEXT NOT NULL, activation_id TEXT NOT NULL, record TEXT NOT NULL,
                 PRIMARY KEY(plugin_id,session_id,activation_id));
             CREATE TABLE IF NOT EXISTS operations (
                 id TEXT PRIMARY KEY, plugin_id TEXT NOT NULL, operation TEXT NOT NULL,
                 status TEXT NOT NULL, error TEXT NOT NULL DEFAULT '');"
        ).map_err(|error| tembra::RuntimeError::unavailable("plugin_administration_storage", error.to_string()))
        },
        validate: |connection| {
            for query in [
                "SELECT singleton,declared FROM composition LIMIT 0",
                "SELECT plugin_id,version,package FROM plugin_versions LIMIT 0",
                "SELECT plugin_id,version,desired,observed,error,activation_id FROM installed_plugins LIMIT 0",
                "SELECT provider_id,configuration FROM provider_configuration LIMIT 0",
                "SELECT plugin_id,session_id,activation_id,record FROM browser_sessions LIMIT 0",
                "SELECT id,plugin_id,operation,status,error FROM operations LIMIT 0",
            ] {
                connection.prepare(query).map_err(|error| {
                    tembra::RuntimeError::unavailable(
                        "plugin_administration_storage",
                        error.to_string(),
                    )
                })?;
            }
            Ok(())
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owning_connection_uses_normal_wal_after_open_and_reopen() -> Result<()> {
        let directory = tempfile::tempdir().expect("test directory");
        let database = directory.path().join("administration.sqlite");
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
}
