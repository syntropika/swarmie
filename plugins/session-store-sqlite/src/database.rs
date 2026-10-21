use rusqlite::{Connection, OpenFlags};
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
        crate::schema::admit(&mut connection)?;
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
        crate::schema::migrate(&mut connection)?;
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

/// Stored rows were admitted canonically. Refuse corrupted JSON rather than silently accepting
/// duplicate object fields, unsafe numeric values or unsupported portable structure.
pub(crate) fn decode<T: tembra::serde::de::DeserializeOwned>(encoded: &str) -> Result<T> {
    let value = tembra::contracts::parse_json(encoded).map_err(corrupt)?;
    serde_json::from_value(value).map_err(corrupt)
}
