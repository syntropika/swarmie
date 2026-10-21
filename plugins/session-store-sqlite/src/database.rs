use rusqlite::{Connection, OpenFlags, TransactionBehavior};
use std::{
    cell::RefCell,
    fs::{File, OpenOptions, TryLockError},
    path::Path,
    time::Duration,
};
use swarmie_session_contracts::{MAX_SEQUENCE, StoreHead};
use tembra::{Result, RuntimeError};

pub(crate) struct Store {
    connection: RefCell<Option<Connection>>,
    lock: RefCell<Option<File>>,
}

pub(crate) fn io(error: impl std::fmt::Display) -> RuntimeError {
    RuntimeError::unavailable("session_store_io", error.to_string())
}

pub(crate) fn invalid(message: impl Into<String>) -> RuntimeError {
    RuntimeError::invalid("session_store_invalid", message)
}

pub(crate) fn corrupt(message: impl std::fmt::Display) -> RuntimeError {
    RuntimeError::unavailable("session_store_corrupt", message.to_string())
}

pub(crate) fn metadata(connection: &Connection) -> Result<StoreHead> {
    let (store_id, commit_sequence): (String, u64) = connection
        .query_row(
            "SELECT store_id, sequence FROM session_store WHERE singleton=1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(io)?;
    if store_id.is_empty() || commit_sequence > MAX_SEQUENCE {
        return Err(corrupt("invalid store metadata"));
    }
    Ok(StoreHead {
        store_id,
        commit_sequence,
    })
}

impl Store {
    pub(crate) fn open(path: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .map_err(io)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if file.metadata().map_err(io)?.nlink() != 1 {
                return Err(invalid("hard-linked databases are unsupported"));
            }
        }
        let path = path.canonicalize().map_err(io)?;
        let mut lock_path = path.as_os_str().to_owned();
        lock_path.push(".owner");
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(Path::new(&lock_path))
            .map_err(io)?;
        match lock.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => {
                return Err(RuntimeError::unavailable(
                    "session_store_owned",
                    "another live provider owns this database",
                ));
            }
            Err(TryLockError::Error(error)) => return Err(io(error)),
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let opened = lock.metadata().map_err(io)?;
            let named = std::fs::metadata(Path::new(&lock_path)).map_err(io)?;
            if opened.nlink() != 1 || opened.dev() != named.dev() || opened.ino() != named.ino() {
                return Err(invalid("unstable database ownership lock"));
            }
        }
        let mut connection = Connection::open_with_flags(
            &path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(io)?;
        // Refuse external contention promptly instead of blocking Tembra's local executor.
        connection.busy_timeout(Duration::ZERO).map_err(io)?;
        let mode: String = connection
            .query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))
            .map_err(io)?;
        if !mode.eq_ignore_ascii_case("wal") {
            return Err(invalid("WAL is required"));
        }
        connection
            .pragma_update(None, "synchronous", "FULL")
            .map_err(io)?;
        connection
            .pragma_update(None, "foreign_keys", "ON")
            .map_err(io)?;
        migrate(&mut connection)?;
        metadata(&connection)?;
        Ok(Self {
            connection: RefCell::new(Some(connection)),
            lock: RefCell::new(Some(lock)),
        })
    }

    pub(crate) fn with<T>(&self, action: impl FnOnce(&mut Connection) -> Result<T>) -> Result<T> {
        let mut connection = self.connection.borrow_mut();
        action(connection.as_mut().ok_or_else(|| {
            RuntimeError::unavailable("session_store_closed", "storage resource is closed")
        })?)
    }

    pub(crate) fn head(&self) -> Result<StoreHead> {
        self.with(|connection| metadata(connection))
    }

    pub(crate) fn close(&self) -> Result<()> {
        let connection = self.connection.borrow_mut().take();
        if let Some(connection) = connection {
            match connection.close() {
                Ok(()) => {}
                Err((connection, error)) => {
                    *self.connection.borrow_mut() = Some(connection);
                    return Err(io(error));
                }
            }
        }
        self.lock.borrow_mut().take();
        Ok(())
    }
}

fn migrate(connection: &mut Connection) -> Result<()> {
    const APPLICATION: i64 = 0x53575345;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(io)?;
    let application: i64 = transaction
        .pragma_query_value(None, "application_id", |row| row.get(0))
        .map_err(io)?;
    let version: i64 = transaction
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(io)?;
    if (application != 0 && application != APPLICATION)
        || version > 1
        || (version != 0 && application != APPLICATION)
        || (version == 0 && application == APPLICATION)
    {
        return Err(RuntimeError::unavailable(
            "session_store_schema",
            "foreign or unsupported schema; data retained",
        ));
    }
    if version == 0 {
        let tables: i64 = transaction.query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'", [], |row| row.get(0)).map_err(io)?;
        if tables != 0 {
            return Err(invalid(
                "session storage requires a dedicated empty database",
            ));
        }
        transaction.execute_batch(
            "CREATE TABLE session_store(singleton INTEGER PRIMARY KEY CHECK(singleton=1), store_id TEXT NOT NULL, sequence INTEGER NOT NULL CHECK(sequence BETWEEN 0 AND 9007199254740991));
             CREATE TABLE sessions(id TEXT PRIMARY KEY, head INTEGER NOT NULL CHECK(head BETWEEN 0 AND 9007199254740991), metadata TEXT NOT NULL CHECK(length(CAST(metadata AS BLOB))<=65536)) WITHOUT ROWID;
             CREATE TABLE session_events(session_id TEXT NOT NULL REFERENCES sessions(id), sequence INTEGER NOT NULL CHECK(sequence BETWEEN 1 AND 9007199254740991), event_id TEXT NOT NULL, turn_id TEXT, event TEXT NOT NULL CHECK(length(CAST(event AS BLOB))<=262144), PRIMARY KEY(session_id, sequence), UNIQUE(session_id,event_id)) WITHOUT ROWID;
             CREATE INDEX session_events_turn ON session_events(session_id,turn_id,sequence);
             CREATE TABLE session_batches(session_id TEXT NOT NULL REFERENCES sessions(id), batch_id TEXT NOT NULL, request TEXT NOT NULL, receipt TEXT NOT NULL, PRIMARY KEY(session_id,batch_id)) WITHOUT ROWID;
             CREATE TRIGGER immutable_events_update BEFORE UPDATE ON session_events BEGIN SELECT RAISE(ABORT,'immutable event'); END;
             CREATE TRIGGER immutable_events_delete BEFORE DELETE ON session_events BEGIN SELECT RAISE(ABORT,'immutable event'); END;
             CREATE TRIGGER immutable_batches_update BEFORE UPDATE ON session_batches BEGIN SELECT RAISE(ABORT,'immutable receipt'); END;
             CREATE TRIGGER immutable_batches_delete BEFORE DELETE ON session_batches BEGIN SELECT RAISE(ABORT,'immutable receipt'); END;
             CREATE TRIGGER immutable_metadata BEFORE UPDATE OF id,metadata ON sessions BEGIN SELECT RAISE(ABORT,'immutable session'); END;
             CREATE TRIGGER immutable_sessions_delete BEFORE DELETE ON sessions BEGIN SELECT RAISE(ABORT,'retained session'); END;"
        ).map_err(io)?;
        transaction
            .execute(
                "INSERT INTO session_store VALUES(1,?1,0)",
                [uuid::Uuid::new_v4().to_string()],
            )
            .map_err(io)?;
        transaction
            .pragma_update(None, "application_id", APPLICATION)
            .map_err(io)?;
        transaction
            .pragma_update(None, "user_version", 1)
            .map_err(io)?;
    }
    transaction.commit().map_err(io)
}

/// Stored rows were admitted canonically. Refuse corrupted JSON rather than silently accepting
/// duplicate object fields, unsafe numeric values or unsupported portable structure.
pub(crate) fn decode<T: tembra::serde::de::DeserializeOwned>(encoded: &str) -> Result<T> {
    let value = tembra::contracts::parse_json(encoded).map_err(corrupt)?;
    serde_json::from_value(value).map_err(corrupt)
}
