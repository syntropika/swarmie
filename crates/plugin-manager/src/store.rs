use std::{cell::RefCell, path::Path};

use rusqlite::{Connection, OptionalExtension, params};
use swarmie_plugin_contracts::*;

use crate::{ManagerError, Result};

/// A lifecycle-owned SQLite connection. Closing removes access before releasing the driver.
pub(crate) struct Store {
    connection: RefCell<Option<Connection>>,
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
        let connection = Connection::open(path)?;
        connection.execute_batch(
            "PRAGMA foreign_keys=ON;
             PRAGMA journal_mode=WAL;
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
                 status TEXT NOT NULL, error TEXT NOT NULL DEFAULT '');",
        )?;
        Ok(Self {
            connection: RefCell::new(Some(connection)),
        })
    }

    pub fn close(&self) -> Result<()> {
        if let Some(connection) = self.connection.borrow_mut().take() {
            connection.close().map_err(|(_, error)| error)?;
        }
        Ok(())
    }

    fn with<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let borrow = self.connection.borrow();
        f(borrow.as_ref().ok_or(ManagerError::Closed)?)
    }

    /// Retains exact approved bytes without changing the installed version selection.
    pub fn retain_version(&self, package: &AcquiredPackage) -> Result<()> {
        self.with(|connection| {
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
        self.with(|connection| {
            let transaction = connection.unchecked_transaction()?;
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
            transaction.commit()?;
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
        self.with(|connection| {
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
        self.with(|connection| {
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
        self.with(|connection| {
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
        self.with(|connection| {
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
