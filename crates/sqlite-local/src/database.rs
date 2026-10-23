use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use std::{
    cell::RefCell,
    fs::{File, OpenOptions, TryLockError},
    path::{Path, PathBuf},
    rc::Rc,
    time::Duration,
};

const APPLICATION: i64 = 0x53574c44;
const VERSION: i64 = 1;

/// Explicit durable file; no in-memory or URI database can back the local owner.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Configuration {
    pub database: PathBuf,
}

/// Domain-owned current schema operations. Both callbacks are synchronous and bounded.
/// Initialization runs inside the owner's schema transaction; validation must not mutate data.
#[derive(Clone, Copy)]
pub struct Schema {
    pub domain: &'static str,
    pub version: u32,
    pub initialize: fn(&Connection) -> tembra::Result<()>,
    pub validate: fn(&Connection) -> tembra::Result<()>,
}

/// Observable acquisition, format, access and SQLite failures.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("local database configuration is invalid: {0}")]
    Configuration(String),
    #[error("another owner holds the local database")]
    Owned,
    #[error("unsupported local database: {0}")]
    Unsupported(String),
    #[error("local database is closed")]
    Closed,
    #[error("local database is already borrowed by another synchronous operation")]
    Busy,
    #[error("local database filesystem access failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("local SQLite operation failed: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("local schema operation failed: {0}")]
    Domain(tembra::RuntimeError),
}
impl From<Error> for tembra::RuntimeError {
    fn from(error: Error) -> Self {
        if let Error::Domain(error) = error {
            return error;
        }
        let (code, reason) = match &error {
            Error::Configuration(_) => (
                tembra::core::ErrorCode::Invalid,
                "sqlite_local_configuration",
            ),
            Error::Owned => (tembra::core::ErrorCode::Unavailable, "sqlite_local_owned"),
            Error::Unsupported(_) => (
                tembra::core::ErrorCode::UpgradeRequired,
                "sqlite_local_schema",
            ),
            Error::Closed => (tembra::core::ErrorCode::Released, "sqlite_local_closed"),
            Error::Busy => (tembra::core::ErrorCode::Unavailable, "sqlite_local_busy"),
            _ => (tembra::core::ErrorCode::Unavailable, "sqlite_local_io"),
        };
        Self::new(code, reason, error.to_string())
    }
}
struct Inner {
    connection: RefCell<Option<Connection>>,
    lock: RefCell<Option<File>>,
    identity: String,
}

/// Cloneable access to one physical engine. Clones do not open connections or extend usability
/// after the owner's close. A Tembra owner Resource controls runtime cleanup.
#[derive(Clone)]
pub struct Database {
    inner: Rc<Inner>,
}
impl Database {
    /// Acquires the single OS lock, admits the file before journal changes and installs missing
    /// current domains atomically. Runtime composition should open this through [`crate::facet`].
    ///
    /// # Errors
    /// Refuses unsupported formats/versions without modifying their database or journal mode.
    /// Schema or I/O errors retain existing data; filesystem failures can leave an empty file.
    pub fn open(configuration: &Configuration, schemas: Vec<Schema>) -> Result<Self, Error> {
        let path = configuration.database.to_string_lossy();
        if path.is_empty() || path == ":memory:" || path.starts_with("file:") {
            return Err(Error::Configuration(
                "a durable database path is required".into(),
            ));
        }
        let mut domains = std::collections::BTreeSet::new();
        for schema in &schemas {
            if schema.domain.is_empty()
                || schema.domain.len() > 128
                || schema.domain.chars().any(char::is_control)
                || schema.version == 0
                || !domains.insert(schema.domain)
            {
                return Err(Error::Configuration(
                    "schema domains must be distinct bounded names with positive versions".into(),
                ));
            }
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&configuration.database)?;
        reject_hard_links(&file)?;
        let path = configuration.database.canonicalize()?;
        let mut lock_path = path.as_os_str().to_owned();
        lock_path.push(".owner");
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(Path::new(&lock_path))?;
        match lock.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => return Err(Error::Owned),
            Err(TryLockError::Error(error)) => return Err(error.into()),
        }
        verify_lock(&lock, Path::new(&lock_path))?;
        let mut connection = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        connection.busy_timeout(Duration::ZERO)?;
        admit(&connection, &schemas)?;
        let mode: String = connection.query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))?;
        if !mode.eq_ignore_ascii_case("wal") {
            return Err(Error::Unsupported("WAL is required".into()));
        }
        connection.pragma_update(None, "synchronous", "NORMAL")?;
        let synchronization: i64 =
            connection.pragma_query_value(None, "synchronous", |r| r.get(0))?;
        if synchronization != 1 {
            return Err(Error::Unsupported(
                "NORMAL synchronization is required".into(),
            ));
        }
        connection.pragma_update(None, "foreign_keys", "ON")?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let application: i64 =
            transaction.pragma_query_value(None, "application_id", |row| row.get(0))?;
        if application == 0 {
            transaction.execute_batch("CREATE TABLE swarmie_sqlite_metadata(singleton INTEGER PRIMARY KEY CHECK(singleton=1),identity TEXT NOT NULL); CREATE TABLE swarmie_sqlite_domains(domain TEXT PRIMARY KEY,version INTEGER NOT NULL CHECK(version>0)) WITHOUT ROWID;")?;
            transaction.execute(
                "INSERT INTO swarmie_sqlite_metadata VALUES(1,?1)",
                [uuid::Uuid::new_v4().to_string()],
            )?;
            transaction.pragma_update(None, "application_id", APPLICATION)?;
            transaction.pragma_update(None, "user_version", VERSION)?;
        }
        for schema in &schemas {
            let version: Option<u32> = transaction
                .query_row(
                    "SELECT version FROM swarmie_sqlite_domains WHERE domain=?1",
                    [schema.domain],
                    |row| row.get(0),
                )
                .optional()?;
            if version.is_none() {
                (schema.initialize)(&transaction).map_err(Error::Domain)?;
                transaction.execute(
                    "INSERT INTO swarmie_sqlite_domains VALUES(?1,?2)",
                    params![schema.domain, schema.version],
                )?;
            }
            (schema.validate)(&transaction).map_err(Error::Domain)?;
        }
        let identity: String = transaction.query_row(
            "SELECT identity FROM swarmie_sqlite_metadata WHERE singleton=1",
            [],
            |row| row.get(0),
        )?;
        if identity.is_empty() {
            return Err(Error::Unsupported(
                "missing physical database identity".into(),
            ));
        }
        transaction.commit()?;
        connection.set_prepared_statement_cache_capacity(256);
        Ok(Self {
            inner: Rc::new(Inner {
                connection: RefCell::new(Some(connection)),
                lock: RefCell::new(Some(lock)),
                identity,
            }),
        })
    }

    /// Compares actual owner identity without trusting file paths or persisted labels.
    pub fn same_owner(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }

    /// Identifies the physical file owner; domain identities remain independently authoritative.
    pub fn identity(&self) -> &str {
        &self.inner.identity
    }

    /// Runs bounded native domain logic without exposing a connection beyond this callback.
    ///
    /// # Errors
    /// Closed or reentrant conflicting access fails before the callback runs. Callback errors
    /// preserve their domain type; this read/access method supplies no rollback guarantee.
    pub fn with_connection<T, E>(
        &self,
        action: impl FnOnce(&Connection) -> Result<T, E>,
    ) -> Result<T, E>
    where
        E: From<Error>,
    {
        let connection = self
            .inner
            .connection
            .try_borrow()
            .map_err(|_| E::from(Error::Busy))?;
        action(connection.as_ref().ok_or_else(|| E::from(Error::Closed))?)
    }

    /// Runs one synchronous atomic write. No Reference call, await or external effect belongs
    /// inside this callback. Validation errors roll back all SQL changes.
    ///
    /// # Errors
    /// A COMMIT error has unknown outcome: callers must reconcile stable operation identities.
    /// No cache or notification should adopt a candidate before this method succeeds.
    pub fn transaction<T, E>(
        &self,
        action: impl FnOnce(&Connection) -> Result<T, E>,
    ) -> Result<T, E>
    where
        E: From<Error>,
    {
        let mut connection = self
            .inner
            .connection
            .try_borrow_mut()
            .map_err(|_| E::from(Error::Busy))?;
        let connection = connection.as_mut().ok_or_else(|| E::from(Error::Closed))?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| E::from(Error::Database(error)))?;
        let output = action(&transaction)?;
        transaction
            .commit()
            .map_err(|error| E::from(Error::Database(error)))?;
        Ok(output)
    }

    /// Closes SQLite before releasing the OS lock. Success makes every clone unusable; repeated
    /// close is harmless. A failed SQLite close retains both connection and ownership lock.
    pub fn close(&self) -> Result<(), Error> {
        let mut connection = self
            .inner
            .connection
            .try_borrow_mut()
            .map_err(|_| Error::Busy)?;
        if let Some(opened) = connection.take()
            && let Err((opened, error)) = opened.close()
        {
            *connection = Some(opened);
            return Err(error.into());
        }
        self.inner.lock.borrow_mut().take();
        Ok(())
    }
}
fn admit(connection: &Connection, schemas: &[Schema]) -> Result<(), Error> {
    let application: i64 = connection.pragma_query_value(None, "application_id", |r| r.get(0))?;
    let version: i64 = connection.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if application == 0 && version == 0 {
        let count: u64 = connection.query_row(
            "SELECT count(*) FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%'",
            [],
            |r| r.get(0),
        )?;
        if count == 0 {
            return Ok(());
        }
    } else if application == APPLICATION && version == VERSION {
        connection.prepare("SELECT singleton,identity FROM swarmie_sqlite_metadata")?;
        connection.prepare("SELECT domain,version FROM swarmie_sqlite_domains")?;
        let identity: Option<String> = connection
            .query_row(
                "SELECT identity FROM swarmie_sqlite_metadata WHERE singleton=1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        if identity.as_ref().is_none_or(|identity| identity.is_empty()) {
            return Err(Error::Unsupported(
                "missing physical database identity".into(),
            ));
        }
        for schema in schemas {
            let version: Option<u32> = connection
                .query_row(
                    "SELECT version FROM swarmie_sqlite_domains WHERE domain=?1",
                    [schema.domain],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(version) = version {
                if version != schema.version {
                    return Err(Error::Unsupported(format!(
                        "unsupported {} domain version {version}",
                        schema.domain
                    )));
                }
                (schema.validate)(connection).map_err(Error::Domain)?;
            }
        }
        return Ok(());
    }
    Err(Error::Unsupported(
        "foreign or unsupported physical database format".into(),
    ))
}
fn reject_hard_links(file: &File) -> Result<(), Error> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if file.metadata()?.nlink() != 1 {
            return Err(Error::Unsupported(
                "hard-linked database files are unsupported".into(),
            ));
        }
    }
    Ok(())
}
fn verify_lock(file: &File, path: &Path) -> Result<(), Error> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let opened = file.metadata()?;
        let named = std::fs::metadata(path)?;
        if opened.nlink() != 1 || opened.dev() != named.dev() || opened.ino() != named.ino() {
            return Err(Error::Unsupported(
                "unstable database ownership lock".into(),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn configuration(path: &Path) -> Configuration {
        Configuration {
            database: path.to_owned(),
        }
    }

    #[test]
    fn unsupported_physical_metadata_is_refused_before_journal_changes() -> Result<(), Error> {
        for remove in [false, true] {
            let directory = tempfile::tempdir()?;
            let path = directory.path().join("local.sqlite");
            let database = Database::open(&configuration(&path), vec![])?;
            database.close()?;
            let connection = Connection::open(&path)?;
            connection.execute_batch("PRAGMA journal_mode=DELETE;")?;
            if remove {
                connection.execute("DELETE FROM swarmie_sqlite_metadata", [])?;
            } else {
                connection.execute("UPDATE swarmie_sqlite_metadata SET identity=''", [])?;
            }
            connection.close().map_err(|(_, error)| error)?;
            let bytes = std::fs::read(&path)?;
            assert!(matches!(
                Database::open(&configuration(&path), vec![]),
                Err(Error::Unsupported(_))
            ));
            assert_eq!(std::fs::read(&path)?, bytes);
        }
        Ok(())
    }
    #[test]
    fn failed_transaction_rolls_back_and_owner_close_revokes_every_clone() -> Result<(), Error> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("local.sqlite");
        let database = Database::open(&configuration(&path), vec![])?;
        let clone = database.clone();
        database.with_connection(|connection| -> Result<_, Error> {
            connection.execute_batch("CREATE TABLE fixture(value INTEGER NOT NULL);")?;
            Ok(())
        })?;
        let refusal = database.transaction(|connection| -> tembra::Result<()> {
            connection
                .execute("INSERT INTO fixture VALUES(1)", [])
                .map_err(|error| tembra::RuntimeError::from(Error::from(error)))?;
            Err(tembra::RuntimeError::invalid(
                "fixture_refused",
                "declined candidate",
            ))
        });
        assert_eq!(refusal.unwrap_err().reason, "fixture_refused");
        assert_eq!(
            database.with_connection(|connection| -> Result<u64, Error> {
                Ok(connection.query_row("SELECT count(*) FROM fixture", [], |row| row.get(0))?)
            })?,
            0
        );
        assert!(database.same_owner(&clone));
        database.close()?;
        assert!(matches!(
            clone.with_connection(|_| -> Result<(), Error> { Ok(()) }),
            Err(Error::Closed)
        ));
        let reopened = Database::open(&configuration(&path), vec![])?;
        assert_eq!(reopened.identity(), database.identity());
        assert!(!reopened.same_owner(&database));
        reopened.close()
    }
}
