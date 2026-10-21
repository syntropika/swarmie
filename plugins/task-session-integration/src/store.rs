use std::{
    cell::RefCell,
    fs::{File, OpenOptions, TryLockError},
    path::Path,
    time::Duration,
};

use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};
use swarmie_task_session_contracts::{
    BindingAdmission, BindingDisposition, BindingKey, TaskSessionBinding,
};
use tembra::{Result, RuntimeError};

use crate::Configuration;

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
}

impl Store {
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
            .pragma_update(None, "synchronous", "FULL")
            .map_err(io)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(io)?;
        transaction.execute_batch("CREATE TABLE IF NOT EXISTS binding_configuration (singleton INTEGER PRIMARY KEY CHECK(singleton=1), task_store_id TEXT NOT NULL, session_store_id TEXT NOT NULL, consumer_id TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS task_session_bindings (task_store_id TEXT NOT NULL, task_id TEXT NOT NULL, disposition TEXT NOT NULL, PRIMARY KEY(task_store_id,task_id)) WITHOUT ROWID;").map_err(io)?;
        transaction
            .execute(
                "INSERT OR IGNORE INTO binding_configuration VALUES(1,?1,?2,?3)",
                params![
                    configuration.task_store_id,
                    configuration.session_store_id,
                    configuration.consumer_id
                ],
            )
            .map_err(io)?;
        let pins: (String,String,String) = transaction.query_row("SELECT task_store_id,session_store_id,consumer_id FROM binding_configuration WHERE singleton=1", [], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(io)?;
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
        })
    }

    fn with<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
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

    pub fn activate(&self, mut binding: TaskSessionBinding) -> Result<TaskSessionBinding> {
        binding.admission = BindingAdmission::Active;
        self.with(|connection| {
            connection.execute("UPDATE task_session_bindings SET disposition=?3 WHERE task_store_id=?1 AND task_id=?2", params![binding.key.task_store_id,binding.key.task_id,canonical(&BindingDisposition::Routed { binding:Box::new(binding.clone()) })?]).map_err(io)?;
            Ok(binding)
        })
    }

    pub fn close(&self) -> Result<()> {
        if let Some(connection) = self.connection.borrow_mut().take() {
            connection.close().map_err(|(_, e)| io(e))?;
        }
        self.lock.borrow_mut().take();
        Ok(())
    }
}

fn read(connection: &Connection, key: &BindingKey) -> Result<Option<BindingDisposition>> {
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

pub(crate) fn canonical(value: &impl serde::Serialize) -> Result<String> {
    let value = tembra::contracts::to_portable_value(value).map_err(io)?;
    tembra::contracts::canonical_json(&value).map_err(io)
}
