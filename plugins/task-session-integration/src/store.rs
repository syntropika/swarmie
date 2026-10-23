use std::{
    cell::{Cell, RefCell},
    fs::{File, OpenOptions, TryLockError},
    path::Path,
    time::Duration,
};

use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};
use swarmie_task_session_contracts::{
    BindingAdmission, BindingDisposition, BindingKey, TaskSessionBinding,
};
use tembra::{Result, RuntimeError};

use crate::{Configuration, SharedConfiguration};

const APPLICATION_ID: i64 = 0x53574249;

pub(crate) fn io(error: impl std::fmt::Display) -> RuntimeError {
    RuntimeError::unavailable("binding_io", error.to_string())
}

pub(crate) fn conflict() -> RuntimeError {
    RuntimeError::invalid(
        "binding_conflict",
        "this source identity already has a different immutable disposition",
    )
}

pub(crate) struct Store {
    connection: RefCell<Option<Connection>>,
    lock: RefCell<Option<File>>,
    shared: Option<swarmie_sqlite_local::Database>,
    closed: Cell<bool>,
}

impl Store {
    /// Admits the bridge's immutable pins using the owner's single connection. The schema must
    /// already be registered with that owner. Failure retains prior pins and dispositions.
    pub fn open_shared(
        database: swarmie_sqlite_local::Database,
        configuration: &SharedConfiguration,
    ) -> Result<Self> {
        configuration.validate()?;
        database.transaction(|connection| {
            validate_schema(connection)?;
            configure(connection, configuration)
        })?;
        Ok(Self {
            connection: RefCell::new(None),
            lock: RefCell::new(None),
            shared: Some(database),
            closed: Cell::new(false),
        })
    }

    pub fn open(configuration: &Configuration) -> Result<Self> {
        configuration.validate()?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&configuration.database)
            .map_err(io)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if file.metadata().map_err(io)?.nlink() != 1 {
                return Err(io("hard-linked binding databases are unsupported"));
            }
        }
        let path = configuration.database.canonicalize().map_err(io)?;
        let mut name = path.as_os_str().to_owned();
        name.push(".owner");
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(Path::new(&name))
            .map_err(io)?;
        match lock.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => {
                return Err(RuntimeError::unavailable(
                    "binding_owned",
                    "another provider owns this binding database",
                ));
            }
            Err(TryLockError::Error(error)) => return Err(io(error)),
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let opened = lock.metadata().map_err(io)?;
            let named = std::fs::metadata(Path::new(&name)).map_err(io)?;
            if opened.nlink() != 1 || opened.dev() != named.dev() || opened.ino() != named.ino() {
                return Err(io("unstable binding ownership lock"));
            }
        }
        let mut connection = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(io)?;
        connection.busy_timeout(Duration::ZERO).map_err(io)?;
        let application: i64 = connection
            .pragma_query_value(None, "application_id", |r| r.get(0))
            .map_err(io)?;
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .map_err(io)?;
        let tables: u64 = connection
            .query_row(
                "SELECT count(*) FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%'",
                [],
                |r| r.get(0),
            )
            .map_err(io)?;
        if !((application == 0 && version == 0 && tables == 0)
            || (application == APPLICATION_ID && version == 1))
        {
            return Err(RuntimeError::invalid(
                "binding_schema",
                "unsupported or unrelated binding database",
            ));
        }
        let mode: String = connection
            .query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))
            .map_err(io)?;
        if !mode.eq_ignore_ascii_case("wal") {
            return Err(io("WAL is required"));
        }
        connection
            .pragma_update(None, "synchronous", "NORMAL")
            .map_err(io)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(io)?;
        initialize_schema(&transaction)?;
        configure(&transaction, &configuration.shared())?;
        transaction
            .pragma_update(None, "application_id", APPLICATION_ID)
            .map_err(io)?;
        transaction
            .pragma_update(None, "user_version", 1)
            .map_err(io)?;
        transaction.commit().map_err(io)?;
        Ok(Self {
            connection: RefCell::new(Some(connection)),
            lock: RefCell::new(Some(lock)),
            shared: None,
            closed: Cell::new(false),
        })
    }

    fn with<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        if self.closed.get() {
            return Err(RuntimeError::unavailable(
                "binding_closed",
                "binding Resource is closed",
            ));
        }
        if let Some(database) = &self.shared {
            return database.with_connection(f);
        }
        let connection = self.connection.borrow();
        f(connection.as_ref().ok_or_else(|| {
            RuntimeError::unavailable("binding_closed", "binding Resource is closed")
        })?)
    }

    pub fn lookup(&self, key: &BindingKey) -> Result<Option<BindingDisposition>> {
        self.with(|connection| read(connection, key))
    }

    pub fn insert(
        &self,
        key: &BindingKey,
        disposition: BindingDisposition,
    ) -> Result<BindingDisposition> {
        self.with(|connection| {
            if let Some(existing) = read(connection, key)? {
                let mut comparison = existing.clone();
                if let BindingDisposition::Routed { binding } = &mut comparison {
                    binding.admission = BindingAdmission::Pending;
                }
                if canonical(&comparison)? != canonical(&disposition)? {
                    return Err(conflict());
                }
                return Ok(existing);
            }
            connection
                .execute(
                    "INSERT INTO task_session_bindings VALUES(?1,?2,?3)",
                    params![key.task_store_id, key.task_id, canonical(&disposition)?],
                )
                .map_err(io)?;
            Ok(disposition)
        })
    }

    pub fn activate(&self, binding: TaskSessionBinding) -> Result<TaskSessionBinding> {
        self.with(|connection| activate_in(connection, binding))
    }

    pub fn close(&self) -> Result<()> {
        if self.shared.is_some() {
            self.closed.set(true);
            return Ok(());
        }
        let connection = self.connection.borrow_mut().take();
        if let Some(connection) = connection
            && let Err((connection, error)) = connection.close()
        {
            *self.connection.borrow_mut() = Some(connection);
            return Err(io(error));
        }
        self.closed.set(true);
        self.lock.borrow_mut().take();
        Ok(())
    }
}

pub(crate) fn initialize_schema(connection: &Connection) -> Result<()> {
    connection.execute_batch("CREATE TABLE IF NOT EXISTS binding_configuration (singleton INTEGER PRIMARY KEY CHECK(singleton=1), task_store_id TEXT NOT NULL, session_store_id TEXT NOT NULL, consumer_id TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS task_session_bindings (task_store_id TEXT NOT NULL, task_id TEXT NOT NULL, disposition TEXT NOT NULL, PRIMARY KEY(task_store_id,task_id)) WITHOUT ROWID;").map_err(io)
}

pub(crate) fn validate_schema(connection: &Connection) -> Result<()> {
    for sql in [
        "SELECT singleton,task_store_id,session_store_id,consumer_id FROM binding_configuration LIMIT 0",
        "SELECT task_store_id,task_id,disposition FROM task_session_bindings LIMIT 0",
    ] {
        connection.prepare(sql).map_err(io)?;
    }
    Ok(())
}

fn configure(connection: &Connection, configuration: &SharedConfiguration) -> Result<()> {
    connection
        .execute(
            "INSERT OR IGNORE INTO binding_configuration VALUES(1,?1,?2,?3)",
            params![
                configuration.task_store_id,
                configuration.session_store_id,
                configuration.consumer_id
            ],
        )
        .map_err(io)?;
    let pins: (String, String, String) = connection.query_row(
        "SELECT task_store_id,session_store_id,consumer_id FROM binding_configuration WHERE singleton=1",
        [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    ).map_err(io)?;
    if pins
        != (
            configuration.task_store_id.clone(),
            configuration.session_store_id.clone(),
            configuration.consumer_id.clone(),
        )
    {
        return Err(RuntimeError::invalid(
            "binding_store_mismatch",
            "binding database is pinned to another store pair or consumer",
        ));
    }
    Ok(())
}

pub(crate) fn read(
    connection: &Connection,
    key: &BindingKey,
) -> Result<Option<BindingDisposition>> {
    let encoded: Option<String> = connection
        .query_row(
            "SELECT disposition FROM task_session_bindings WHERE task_store_id=?1 AND task_id=?2",
            params![key.task_store_id, key.task_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(io)?;
    let disposition: Option<BindingDisposition> = encoded
        .map(|value| {
            serde_json::from_str(&value)
                .map_err(|e| RuntimeError::unavailable("binding_corrupt", e.to_string()))
        })
        .transpose()?;
    if let Some(value) = &disposition {
        let stored = match value {
            BindingDisposition::Routed { binding } => &binding.key,
            BindingDisposition::Excluded { exclusion } => &exclusion.key,
        };
        if stored != key {
            return Err(RuntimeError::unavailable(
                "binding_corrupt",
                "stored disposition key differs from its row",
            ));
        }
    }
    Ok(disposition)
}

pub(crate) fn activate_in(
    connection: &Connection,
    mut binding: TaskSessionBinding,
) -> Result<TaskSessionBinding> {
    binding.admission = BindingAdmission::Active;
    connection
        .execute(
            "UPDATE task_session_bindings SET disposition=?3 WHERE task_store_id=?1 AND task_id=?2",
            params![
                binding.key.task_store_id,
                binding.key.task_id,
                canonical(&BindingDisposition::Routed {
                    binding: Box::new(binding.clone())
                })?
            ],
        )
        .map_err(io)?;
    Ok(binding)
}

pub(crate) fn canonical(value: &impl serde::Serialize) -> Result<String> {
    let value = tembra::contracts::to_portable_value(value).map_err(io)?;
    tembra::contracts::canonical_json(&value).map_err(io)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_schema_defers_pins_and_preserves_them_on_mismatched_admission() -> Result<()> {
        let directory = tempfile::tempdir().expect("test directory");
        let database = swarmie_sqlite_local::Database::open(
            &swarmie_sqlite_local::Configuration {
                database: directory.path().join("local.sqlite"),
            },
            vec![crate::sqlite_schema()],
        )
        .map_err(RuntimeError::from)?;
        let configuration = SharedConfiguration {
            task_store_id: "tasks".into(),
            session_store_id: "sessions".into(),
            consumer_id: "history".into(),
            auto_publish: false,
            poll_interval_ms: 2,
        };
        let count = database.with_connection(|connection| {
            connection
                .query_row("SELECT count(*) FROM binding_configuration", [], |row| {
                    row.get::<_, u64>(0)
                })
                .map_err(io)
        })?;
        assert_eq!(
            count, 0,
            "physical schema installation must not invent domain pins"
        );
        let store = Store::open_shared(database.clone(), &configuration)?;
        let key = BindingKey {
            task_store_id: "tasks".into(),
            task_id: "unrelated".into(),
        };
        let disposition = BindingDisposition::Excluded {
            exclusion: swarmie_task_session_contracts::ExcludedBinding {
                key: key.clone(),
                reason: "unrelated task".into(),
            },
        };
        store.insert(&key, disposition.clone())?;
        let mut mismatched = configuration.clone();
        mismatched.session_store_id = "replacement".into();
        let error = match Store::open_shared(database.clone(), &mismatched) {
            Ok(_) => panic!("a replacement destination must not adopt existing pins"),
            Err(error) => error,
        };
        assert_eq!(error.reason, "binding_store_mismatch");
        assert_eq!(store.lookup(&key)?, Some(disposition.clone()));
        store.close()?;
        assert_eq!(store.lookup(&key).unwrap_err().reason, "binding_closed");
        let reopened = Store::open_shared(database.clone(), &configuration)?;
        assert_eq!(reopened.lookup(&key)?, Some(disposition));
        database.close().map_err(RuntimeError::from)?;
        assert!(
            reopened.lookup(&key).is_err(),
            "owner close revokes every borrowed connection"
        );
        reopened.close()?;
        Ok(())
    }

    #[test]
    fn owning_connection_uses_normal_wal_after_open_and_reopen() -> Result<()> {
        let directory = tempfile::tempdir().expect("test directory");
        let configuration = Configuration {
            database: directory.path().join("bindings.sqlite"),
            task_store_id: "tasks".into(),
            session_store_id: "sessions".into(),
            consumer_id: "history".into(),
            auto_publish: false,
            poll_interval_ms: 2,
        };
        for _ in 0..2 {
            let store = Store::open(&configuration)?;
            store.with(|connection| {
                assert_eq!(
                    connection
                        .pragma_query_value(None, "synchronous", |row| row.get::<_, i64>(0))
                        .map_err(io)?,
                    1
                );
                assert_eq!(
                    connection
                        .pragma_query_value(None, "journal_mode", |row| row.get::<_, String>(0))
                        .map_err(io)?,
                    "wal"
                );
                Ok(())
            })?;
            store.close()?;
        }
        Ok(())
    }
}
