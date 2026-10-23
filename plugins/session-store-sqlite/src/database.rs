use rusqlite::{Connection, OpenFlags};
use std::{
    cell::RefCell,
    fs::{File, OpenOptions, TryLockError},
    path::Path,
    time::Duration,
};
use swarmie_session_contracts::{MAX_SEQUENCE, SessionStoreIdentity, StoreHead};
use tembra::{Result, RuntimeError};

/// Holds every fixed and transaction-control statement and the four filtered event-page shapes.
const STATEMENT_CACHE_CAPACITY: usize = 32;

/// An immediate write transaction whose control statements come from the statement cache.
///
/// It keeps the rollback semantics of rusqlite's default guard: dropping it without a successful
/// COMMIT rolls back, including after a COMMIT that failed while SQLite still holds the
/// transaction. Statements borrowing it always end before it drops.
pub(crate) struct Transaction<'c> {
    connection: &'c Connection,
}

impl<'c> Transaction<'c> {
    /// Takes the database write lock immediately; contention fails at once (zero busy timeout).
    pub(crate) fn immediate(connection: &'c Connection) -> Result<Self> {
        control(connection, "BEGIN IMMEDIATE").map_err(io)?;
        Ok(Self { connection })
    }

    pub(crate) fn commit(self) -> Result<()> {
        control(self.connection, "COMMIT").map_err(io)
    }
}

impl std::ops::Deref for Transaction<'_> {
    type Target = Connection;

    fn deref(&self) -> &Connection {
        self.connection
    }
}

impl Drop for Transaction<'_> {
    fn drop(&mut self) {
        if !self.connection.is_autocommit() {
            // Drop cannot report failure; closing the connection also discards the transaction.
            let _ = control(self.connection, "ROLLBACK");
        }
    }
}

fn control(connection: &Connection, sql: &str) -> rusqlite::Result<()> {
    connection.prepare_cached(sql)?.execute([]).map(|_| ())
}

pub(crate) struct Store {
    connection: RefCell<Option<Connection>>,
    lock: RefCell<Option<File>>,
    /// Immutable database identity, read while opening. No operation updates it.
    pub(crate) store_id: String,
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

/// Reads and validates the complete singleton row while admitting or opening a database.
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
        crate::schema::initialize(&mut connection)?;
        let head = metadata(&connection)?;
        connection.set_prepared_statement_cache_capacity(STATEMENT_CACHE_CAPACITY);
        Ok(Self {
            connection: RefCell::new(Some(connection)),
            lock: RefCell::new(Some(lock)),
            store_id: head.store_id,
        })
    }

    pub(crate) fn with<T>(&self, action: impl FnOnce(&mut Connection) -> Result<T>) -> Result<T> {
        let mut connection = self.connection.borrow_mut();
        action(connection.as_mut().ok_or_else(|| {
            RuntimeError::unavailable("session_store_closed", "storage resource is closed")
        })?)
    }

    /// Answers from the identity read while opening, without SQL or publication.
    pub(crate) fn identity(&self) -> Result<SessionStoreIdentity> {
        self.with(|_| {
            Ok(SessionStoreIdentity {
                store_id: self.store_id.clone(),
            })
        })
    }

    /// Current durable watermark: the immutable identity and the committed store sequence.
    pub(crate) fn head(&self) -> Result<StoreHead> {
        self.with(|connection| {
            Ok(StoreHead {
                store_id: self.store_id.clone(),
                commit_sequence: commit_sequence(connection)?,
            })
        })
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

/// Committed store sequence. Writes read it inside their immediate transaction.
pub(crate) fn commit_sequence(connection: &Connection) -> Result<u64> {
    let sequence: u64 = connection
        .prepare_cached("SELECT sequence FROM session_store WHERE singleton=1")
        .and_then(|mut statement| statement.query_row([], |row| row.get(0)))
        .map_err(io)?;
    if sequence > MAX_SEQUENCE {
        return Err(corrupt("invalid store metadata"));
    }
    Ok(sequence)
}

/// Decodes a row that this provider admitted and stored as canonical text with one typed parse.
/// Malformed JSON and duplicate, unknown or mistyped fields fail explicitly as corrupt data.
///
/// The parse keeps serde_json's default recursion limit, which accepts 127 nested containers
/// and bounds recursion on tampered rows. Admission keeps every stored text within it: Tembra
/// admits 128 nested containers: metadata reserves its create/record wrapper, and events
/// reserve their complete page/projection envelope. Stored metadata therefore holds at most
/// 127 containers and stored event envelopes at most 126.
pub(crate) fn decode_stored<T: tembra::serde::de::DeserializeOwned>(encoded: &str) -> Result<T> {
    serde_json::from_str(encoded).map_err(corrupt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use swarmie_session_contracts::{
        AppendEvents, Correlation, CreateSession, EventQuery, NewEvent, SessionLookup,
    };

    fn nested(depth: usize) -> serde_json::Value {
        let mut value = serde_json::Value::Null;
        for _ in 0..depth {
            value = serde_json::json!([value]);
        }
        value
    }

    fn append(id: &str, payload: serde_json::Value) -> AppendEvents {
        AppendEvents {
            session_id: "deep".into(),
            batch_id: id.into(),
            expected_head: None,
            events: vec![NewEvent {
                event_id: id.into(),
                kind: "plugin.deep".into(),
                version: 1,
                correlation: Correlation::default(),
                occurred_at_ms: None,
                payload,
            }],
        }
    }

    #[test]
    fn rows_at_the_admitted_nesting_limit_remain_readable() -> Result<()> {
        let directory = tempfile::tempdir().map_err(io)?;
        let store = Store::open(&directory.path().join("sessions.sqlite"))?;
        // Metadata reserves one create/record wrapper; event payloads reserve the four
        // containers of their returned pages. These are the deepest admitted values, keeping
        // stored metadata at serde's 127-container limit and stored event envelopes at 126.
        let create = CreateSession {
            session_id: "deep".into(),
            metadata: nested(127),
        };
        let created = store.create(create.clone())?;
        assert_eq!(created.metadata, nested(127));
        assert_eq!(
            store.create(create)?,
            created,
            "exact retry decodes the row"
        );
        assert_eq!(
            store.get(SessionLookup {
                session_id: "deep".into()
            })?,
            Some(created)
        );
        let request = append("deep", nested(124));
        let receipt = store.append(request.clone())?;
        assert_eq!(store.append(request.clone())?, receipt);
        let page = store.events(EventQuery::new("deep"))?;
        assert_eq!(page.events.len(), 1);
        assert_eq!(page.events[0].event, request.events[0]);
        // One more container is refused before any mutation.
        let head = store.head()?;
        let deeper = CreateSession {
            session_id: "deeper".into(),
            metadata: nested(128),
        };
        assert_eq!(
            store.create(deeper).unwrap_err().reason,
            "session_store_invalid"
        );
        assert_eq!(
            store
                .append(append("deeper", nested(125)))
                .unwrap_err()
                .reason,
            "session_store_invalid"
        );
        assert_eq!(store.head()?, head);
        store.close()
    }
}
