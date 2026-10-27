use futures::FutureExt;
use rusqlite::Connection;
use std::{
    cell::{Cell, RefCell},
    path::{Path, PathBuf},
    rc::Rc,
    time::Duration,
};
use swarmie_storage_core::{ChangeSource, StorageOwner};

enum OpenError {
    Unavailable(String),
    Schema(String),
    Owner(swarmie_storage_core::OwnerError),
}

pub(crate) struct Database {
    path: PathBuf,
    pub connection: RefCell<Option<Connection>>,
    leases: Cell<usize>,
    pub health: StorageOwner,
    pub tasks: ChangeSource,
}

/// Shares one SQLite connection across domain leases.
/// Construction performs no I/O. After closure or an unknown commit, create a new owner.
#[derive(Clone)]
pub struct SqliteOwner(pub(crate) Rc<Database>);

impl SqliteOwner {
    /// Creates an owner. The first domain lease opens the file.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self(Rc::new(Database {
            path: path.into(),
            connection: RefCell::new(None),
            leases: Cell::new(0),
            health: StorageOwner::default(),
            tasks: ChangeSource::default(),
        }))
    }

    /// Returns this owner's identity and write-health gate.
    pub fn authority(&self) -> StorageOwner {
        self.0.health.clone()
    }

    /// Opens a plugin inventory lease on the shared connection.
    pub fn inventory(
        &self,
    ) -> Result<crate::sqlite::SqliteStorage, swarmie_plugin_manager::StorageError> {
        Ok(crate::sqlite::SqliteStorage {
            lease: self.lease().map_err(|e| match e {
                OpenError::Unavailable(e) => swarmie_plugin_manager::StorageError::Unavailable(e),
                OpenError::Schema(e) => swarmie_plugin_manager::StorageError::InvalidInventory(e),
                OpenError::Owner(e) => swarmie_plugin_manager::StorageError::Owner(e),
            })?,
        })
    }

    /// Opens a task lease on the shared connection.
    pub fn tasks(
        &self,
    ) -> Result<crate::sqlite::SqliteTaskStorage, swarmie_durable_tasks::StorageError> {
        Ok(crate::sqlite::SqliteTaskStorage {
            lease: self.lease().map_err(|e| match e {
                OpenError::Unavailable(e) => swarmie_durable_tasks::StorageError::Unavailable(e),
                OpenError::Schema(e) => swarmie_durable_tasks::StorageError::Invalid(e),
                OpenError::Owner(e) => swarmie_durable_tasks::StorageError::Owner(e),
            })?,
        })
    }

    /// Opens a conversation lease on the shared connection.
    pub fn conversations(
        &self,
    ) -> Result<crate::sqlite::SqliteConversationStorage, swarmie_conversations::StorageError> {
        Ok(crate::sqlite::SqliteConversationStorage {
            lease: self.lease().map_err(|e| match e {
                OpenError::Unavailable(e) => swarmie_conversations::StorageError::Unavailable(e),
                OpenError::Schema(e) => swarmie_conversations::StorageError::Invalid(e),
                OpenError::Owner(e) => swarmie_conversations::StorageError::Owner(e),
            })?,
        })
    }

    /// Opens conversation leases during managed resource activation.
    pub fn conversation_factory(&self) -> swarmie_conversations::StorageFactory {
        let owner = self.clone();
        Rc::new(move || {
            let owner = owner.clone();
            async move {
                Ok(Rc::new(owner.conversations()?)
                    as Rc<dyn swarmie_conversations::ConversationStorage>)
            }
            .boxed_local()
        })
    }

    /// Opens inventory leases during managed resource activation.
    /// After the last lease closes, create a new owner and factory.
    pub fn inventory_factory(&self) -> swarmie_plugin_manager::StorageFactory {
        let owner = self.clone();
        Rc::new(move || {
            let owner = owner.clone();
            async move {
                Ok(Rc::new(owner.inventory()?) as Rc<dyn swarmie_plugin_manager::PluginStorage>)
            }
            .boxed_local()
        })
    }

    /// Opens task leases during managed resource activation.
    /// Activation retry after final closure requires a new owner.
    pub fn task_factory(&self) -> swarmie_durable_tasks::StorageFactory {
        let owner = self.clone();
        Rc::new(move || {
            let owner = owner.clone();
            async move { Ok(Rc::new(owner.tasks()?) as Rc<dyn swarmie_durable_tasks::TaskStorage>) }
                .boxed_local()
        })
    }

    fn lease(&self) -> Result<Lease, OpenError> {
        self.0.health.check().map_err(OpenError::Owner)?;
        if self.0.connection.borrow().is_none() {
            *self.0.connection.borrow_mut() = Some(open(&self.0.path)?);
        }
        self.0.leases.set(self.0.leases.get() + 1);
        Ok(Lease {
            database: self.0.clone(),
            released: Cell::new(false),
        })
    }
}

pub(crate) struct Lease {
    pub database: Rc<Database>,
    released: Cell<bool>,
}

impl Lease {
    pub fn check(&self) -> Result<(), swarmie_storage_core::OwnerError> {
        if self.released.get() {
            Err(swarmie_storage_core::OwnerError::Closed)
        } else {
            Ok(())
        }
    }

    pub fn close(&self) -> Result<(), String> {
        if self.released.get() {
            return Ok(());
        }
        if self.database.leases.get() == 1 {
            let connection = self.database.connection.borrow_mut().take();
            if let Some(connection) = connection
                && let Err((connection, error)) = connection.close()
            {
                *self.database.connection.borrow_mut() = Some(connection);
                return Err(error.to_string());
            }
        }
        self.released.set(true);
        self.database.leases.set(self.database.leases.get() - 1);
        if self.database.leases.get() == 0 {
            self.database.health.closed();
        }
        Ok(())
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        if self.close().is_err() {
            self.database.health.require_recovery();
        }
    }
}

fn open(path: &Path) -> Result<Connection, OpenError> {
    let connection = Connection::open(path).map_err(|e| OpenError::Unavailable(e.to_string()))?;
    connection
        .busy_timeout(Duration::from_secs(5))
        .map_err(|e| OpenError::Unavailable(e.to_string()))?;
    let mode: String = connection
        .query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))
        .map_err(|e| OpenError::Unavailable(e.to_string()))?;
    connection
        .pragma_update(None, "synchronous", "NORMAL")
        .map_err(|e| OpenError::Unavailable(e.to_string()))?;
    let synchronous: u32 = connection
        .query_row("PRAGMA synchronous", [], |r| r.get(0))
        .map_err(|e| OpenError::Unavailable(e.to_string()))?;
    if !mode.eq_ignore_ascii_case("wal") || synchronous != 1 {
        return Err(OpenError::Unavailable(
            "the connection requires WAL and synchronous=NORMAL".into(),
        ));
    }
    connection.execute_batch("BEGIN IMMEDIATE;
        CREATE TABLE IF NOT EXISTS swarmie_plugins_meta(singleton INTEGER PRIMARY KEY CHECK(singleton=1),schema_version INTEGER NOT NULL,revision INTEGER NOT NULL CHECK(revision>=0));
        INSERT OR IGNORE INTO swarmie_plugins_meta VALUES(1,1,0);
        CREATE TABLE IF NOT EXISTS swarmie_plugins(id TEXT PRIMARY KEY NOT NULL,version INTEGER NOT NULL CHECK(version>0 AND version<=4294967295),enabled INTEGER NOT NULL CHECK(enabled IN (0,1)));
        CREATE TABLE IF NOT EXISTS swarmie_tasks_meta(singleton INTEGER PRIMARY KEY CHECK(singleton=1),schema_version INTEGER NOT NULL,sequence INTEGER NOT NULL CHECK(sequence>=0));
        INSERT OR IGNORE INTO swarmie_tasks_meta VALUES(1,3,0);
        CREATE TABLE IF NOT EXISTS swarmie_tasks(id TEXT PRIMARY KEY NOT NULL,data TEXT NOT NULL,progress TEXT NOT NULL,assignment TEXT,service TEXT NOT NULL,version INTEGER NOT NULL,principal TEXT NOT NULL,revision INTEGER NOT NULL,sequence INTEGER NOT NULL UNIQUE,created_at INTEGER NOT NULL,state TEXT NOT NULL,expires_at INTEGER,wake_at INTEGER);
        CREATE INDEX IF NOT EXISTS swarmie_tasks_ready ON swarmie_tasks(state,sequence);
        CREATE INDEX IF NOT EXISTS swarmie_tasks_principal ON swarmie_tasks(principal,sequence);
        CREATE TABLE IF NOT EXISTS swarmie_conversations_meta(singleton INTEGER PRIMARY KEY CHECK(singleton=1),schema_version INTEGER NOT NULL);
        INSERT OR IGNORE INTO swarmie_conversations_meta VALUES(1,1);
        CREATE TABLE IF NOT EXISTS swarmie_conversations(id TEXT PRIMARY KEY NOT NULL,principal TEXT NOT NULL,parent TEXT,parent_entry TEXT,head TEXT,CHECK((parent IS NULL AND parent_entry IS NULL) OR (parent IS NOT NULL AND parent_entry IS NOT NULL AND head IS NOT NULL)));
        CREATE TABLE IF NOT EXISTS swarmie_conversation_entries(id TEXT PRIMARY KEY NOT NULL,conversation TEXT NOT NULL,position INTEGER NOT NULL CHECK(position>0),data TEXT NOT NULL,UNIQUE(conversation,position));
        COMMIT;").map_err(|e| OpenError::Unavailable(e.to_string()))?;
    connection.execute_batch("CREATE INDEX IF NOT EXISTS swarmie_tasks_wake ON swarmie_tasks(wake_at,sequence) WHERE state='sleeping';")
        .map_err(|e| OpenError::Unavailable(e.to_string()))?;
    connection.execute_batch("CREATE TABLE IF NOT EXISTS swarmie_task_signals(task TEXT NOT NULL,key TEXT NOT NULL,value TEXT NOT NULL,consumed INTEGER NOT NULL CHECK(consumed IN (0,1)),PRIMARY KEY(task,key)) WITHOUT ROWID;")
        .map_err(|e| OpenError::Unavailable(e.to_string()))?;
    for (table, expected) in [
        ("swarmie_plugins_meta", 1),
        ("swarmie_tasks_meta", 3),
        ("swarmie_conversations_meta", 1),
    ] {
        let version: u32 = connection
            .query_row(
                &format!("SELECT schema_version FROM {table} WHERE singleton=1"),
                [],
                |r| r.get(0),
            )
            .map_err(|e| OpenError::Unavailable(e.to_string()))?;
        if version != expected {
            return Err(OpenError::Schema(format!(
                "unsupported {table} schema {version}"
            )));
        }
    }
    Ok(connection)
}

pub(crate) fn invalid_sql(error: &rusqlite::Error) -> bool {
    use rusqlite::{Error, ErrorCode};
    matches!(
        error,
        Error::ToSqlConversionFailure(_)
            | Error::FromSqlConversionFailure(..)
            | Error::IntegralValueOutOfRange(..)
            | Error::InvalidColumnType(..)
            | Error::Utf8Error(..)
            | Error::SqliteFailure(
                rusqlite::ffi::Error {
                    code: ErrorCode::ConstraintViolation
                        | ErrorCode::TooBig
                        | ErrorCode::TypeMismatch,
                    ..
                },
                _
            )
    )
}
