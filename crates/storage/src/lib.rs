mod bootstrap;
mod local;
mod nodes;
use async_trait::async_trait;
pub use bootstrap::default_data_root;
pub use nodes::{NodeCatalog, NodeConnection, NodeRecord, NodeTransport};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use swarmie::*;

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

const APPLICATION_ID: i32 = 0x5357524d;
const SCHEMA_VERSION: i32 = 3;

pub type StoreIdentity = LocalStoreIdentity;

/// Canonical host state. Blocking SQLite work runs outside async executor threads.
#[derive(Clone)]
pub struct SqliteSessionStore {
    root: PathBuf,
    path: PathBuf,
    connection: Arc<Mutex<Connection>>,
    identity: StoreIdentity,
    #[cfg(unix)]
    file_identity: (u64, u64),
}
fn database_error(error: rusqlite::Error) -> Error {
    Error::Storage(error.to_string())
}
fn validate_id(id: &str) -> Result<()> {
    uuid::Uuid::parse_str(id).map_err(|_| Error::Invalid("session id must be a UUID".into()))?;
    Ok(())
}
fn integer(value: u64) -> Result<i64> {
    i64::try_from(value).map_err(|_| Error::Invalid("storage integer exceeds SQLite range".into()))
}
impl SqliteSessionStore {
    pub async fn new(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        tokio::task::spawn_blocking(move || Self::open(root))
            .await
            .map_err(|e| Error::Storage(format!("store initialization failed: {e}")))?
    }
    fn open(root: PathBuf) -> Result<Self> {
        bootstrap::private_directory(&root)?;
        let root = std::fs::canonicalize(root)?;
        bootstrap::private_directory(&root.join("runtime"))?;
        let lock_path = root.join("runtime/bootstrap.lock");
        bootstrap::regular_file(&lock_path)?;
        let mut options = std::fs::OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let lock = options.open(lock_path)?;
        fs2::FileExt::lock_exclusive(&lock)?;
        let settings = bootstrap::load(&root)?;
        let state = root.join("state");
        let path = state.join("host.sqlite");
        let exists = bootstrap::regular_file(&path)?;
        if settings.is_some() && !exists {
            return Err(Error::Storage("configured host database is missing; restore it or deliberately select a fresh root".into()));
        }
        if !exists {
            bootstrap::reject_legacy_root(&root)?;
        }
        bootstrap::private_directory(&state)?;
        if !exists {
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            options.open(&path)?.sync_all()?;
            bootstrap::sync_directory(&state)?;
        }
        for suffix in ["", "-wal", "-shm"] {
            let auxiliary = state.join(format!("host.sqlite{suffix}"));
            if bootstrap::regular_file(&auxiliary)? {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(auxiliary, std::fs::Permissions::from_mode(0o600))?;
                }
            }
        }
        let mut connection =
            Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE)
                .map_err(database_error)?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(database_error)?;
        if exists {
            let app: i32 = connection
                .pragma_query_value(None, "application_id", |row| row.get(0))
                .map_err(database_error)?;
            let version: i32 = connection
                .pragma_query_value(None, "user_version", |row| row.get(0))
                .map_err(database_error)?;
            if app != APPLICATION_ID || version != SCHEMA_VERSION {
                return Err(Error::Storage("unsupported host database; select a fresh development root; existing data was preserved".into()));
            }
        }
        connection
            .execute_batch(
                "PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON;",
            )
            .map_err(database_error)?;
        let journal: String = connection
            .pragma_query_value(None, "journal_mode", |row| row.get(0))
            .map_err(database_error)?;
        if journal != "wal" {
            return Err(Error::Storage(
                "host database requires SQLite WAL support".into(),
            ));
        }
        if !exists {
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(database_error)?;
            transaction
                .execute_batch(include_str!("schema.sql"))
                .map_err(database_error)?;
            transaction
                .execute(
                    "INSERT INTO store_identity VALUES (1, ?1, ?2, ?3, ?4)",
                    params![
                        uuid::Uuid::new_v4().to_string(),
                        uuid::Uuid::new_v4().to_string(),
                        uuid::Uuid::new_v4().to_string(),
                        integer(
                            std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_millis() as u64
                        )?
                    ],
                )
                .map_err(database_error)?;
            transaction
                .pragma_update(None, "application_id", APPLICATION_ID)
                .map_err(database_error)?;
            transaction
                .pragma_update(None, "user_version", SCHEMA_VERSION)
                .map_err(database_error)?;
            transaction.commit().map_err(database_error)?;
            bootstrap::sync_directory(&state)?;
        }
        let identity = connection.query_row("SELECT store_id, environment_id, owner_scope_id FROM store_identity WHERE singleton=1", [], |row| {
            Ok(StoreIdentity { store_id: row.get(0)?, environment_id: row.get(1)?, owner_scope_id: row.get(2)? })
        }).map_err(database_error)?;
        for id in [
            &identity.store_id,
            &identity.environment_id,
            &identity.owner_scope_id,
        ] {
            validate_id(id)?;
        }
        if settings
            .as_ref()
            .is_some_and(|s| s.store_id != identity.store_id)
        {
            return Err(Error::Storage(
                "host store identity differs from bootstrap settings".into(),
            ));
        }
        if settings.is_none() {
            bootstrap::save(&root, &identity.store_id)?;
        }
        #[cfg(unix)]
        let file_identity = {
            use std::os::unix::fs::MetadataExt;
            let meta = std::fs::metadata(&path)?;
            (meta.dev(), meta.ino())
        };
        Ok(Self {
            root,
            path,
            connection: Arc::new(Mutex::new(connection)),
            identity,
            #[cfg(unix)]
            file_identity,
        })
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn database_path(&self) -> &Path {
        &self.path
    }
    pub fn identity(&self) -> &StoreIdentity {
        &self.identity
    }
    async fn database<T: Send + 'static>(
        &self,
        operation: impl FnOnce(&mut Connection) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let store = self.clone();
        tokio::task::spawn_blocking(move || {
            let mut connection = store
                .connection
                .lock()
                .map_err(|_| Error::Storage("host store lock poisoned".into()))?;
            if !bootstrap::regular_file(&store.path)? {
                return Err(Error::Storage("host database is missing".into()));
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                let meta = std::fs::metadata(&store.path)?;
                if (meta.dev(), meta.ino()) != store.file_identity {
                    return Err(Error::Storage(
                        "host database was replaced; reconnect to the verified store".into(),
                    ));
                }
            }
            operation(&mut connection)
        })
        .await
        .map_err(|e| Error::Storage(format!("host store operation failed: {e}")))?
    }
}
#[async_trait]
impl SessionStore for SqliteSessionStore {
    async fn local_catalog(&self) -> Result<LocalCatalog> {
        self.catalog().await
    }
    async fn update_catalog(&self, update: CatalogUpdate) -> Result<CatalogRecord> {
        self.mutate_catalog(update).await
    }
    async fn prepare_context(
        &self,
        session_id: &str,
        binding_id: &str,
        selection: ContextSelection,
    ) -> Result<WorkingContext> {
        self.prepare(session_id, binding_id, selection).await
    }
    async fn working_context(&self, id: &str) -> Result<Option<WorkingContext>> {
        self.context(id).await
    }
    async fn extension_state(&self, id: &str) -> Result<Option<serde_json::Value>> {
        self.read_extension(id).await
    }
    async fn save_extension_state(&self, id: &str, value: serde_json::Value) -> Result<()> {
        self.write_extension(id, value).await
    }
    fn transcript_durability(&self) -> Option<&'static str> {
        Some("per-event")
    }
    async fn commit(&self, write: SessionWrite<'_>) -> Result<()> {
        let id = write.session_id()?.to_owned();
        validate_id(&id)?;
        if let Some(snapshot) = write.snapshot
            && let Some(context) = &snapshot.working_context
            && (context.session_id != id || snapshot.options.cwd != context.directory.path)
        {
            return Err(Error::Storage(
                "snapshot differs from its retained working context".into(),
            ));
        }
        let context = write.snapshot.and_then(|s| s.working_context.clone());
        let snapshot = write.snapshot.map(serde_json::to_string).transpose()?;
        let record = write
            .submission
            .map(|r| {
                Ok::<_, Error>((
                    r.receipt.idempotency_key.clone(),
                    r.receipt.id.clone(),
                    serde_json::to_string(r)?,
                ))
            })
            .transpose()?;
        let events = write
            .events
            .iter()
            .map(|e| Ok::<_, Error>((integer(e.sequence)?, serde_json::to_string(e)?)))
            .collect::<Result<Vec<_>>>()?;
        self.database(move |connection| {
            let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(database_error)?;
            transaction.execute("INSERT INTO sessions(session_id) VALUES (?1) ON CONFLICT DO NOTHING", [&id]).map_err(database_error)?;
            if snapshot.is_some() {
                let retained: Option<String> = transaction.query_row("SELECT record_json FROM working_contexts WHERE session_id=?1", [&id], |r| r.get(0)).optional().map_err(database_error)?;
                if retained.map(|j| serde_json::from_str::<WorkingContext>(&j)).transpose()? != context {
                    return Err(Error::Storage("working context is missing or differs from its retained record".into()));
                }
            }
            if let Some(snapshot) = snapshot {
                transaction.execute("UPDATE sessions SET snapshot_json=?2 WHERE session_id=?1", params![id, snapshot]).map_err(database_error)?;
            }
            if let Some((key, submission_id, json)) = record {
                transaction.execute("INSERT INTO submission_records VALUES (?1, ?2, ?3, ?4) ON CONFLICT(session_id, request_key) DO UPDATE SET submission_id=excluded.submission_id, record_json=excluded.record_json",
                    params![id, key, submission_id, json]).map_err(database_error)?;
            }
            for (sequence, json) in events {
                transaction.execute("INSERT INTO session_events VALUES (?1, ?2, ?3)", params![id, sequence, json]).map_err(database_error)?;
            }
            transaction.commit().map_err(database_error)
        }).await
    }
    async fn load(&self, id: &str) -> Result<Option<SessionSnapshot>> {
        validate_id(id)?;
        let id = id.to_owned();
        self.database(move |connection| {
            let json: Option<String> = connection.query_row("SELECT snapshot_json FROM sessions WHERE session_id=?1 AND snapshot_json IS NOT NULL", [&id], |row| row.get(0)).optional().map_err(database_error)?;
            json.map(|s| serde_json::from_str(&s).map_err(Error::from)).transpose()
        }).await
    }
    async fn list(&self) -> Result<Vec<SessionSnapshot>> {
        self.database(|connection| {
            let mut statement = connection.prepare("SELECT snapshot_json FROM sessions WHERE snapshot_json IS NOT NULL ORDER BY session_id").map_err(database_error)?;
            let rows = statement.query_map([], |row| row.get::<_, String>(0)).map_err(database_error)?;
            rows.map(|row| Ok(serde_json::from_str(&row.map_err(database_error)?)?)).collect()
        }).await
    }
    async fn submissions(&self, id: &str) -> Result<Vec<SubmissionRecord>> {
        validate_id(id)?;
        let id = id.to_owned();
        self.database(move |connection| {
            let mut statement = connection.prepare("SELECT record_json FROM submission_records WHERE session_id=?1 ORDER BY request_key").map_err(database_error)?;
            let rows = statement.query_map([id], |row| row.get::<_, String>(0)).map_err(database_error)?;
            rows.map(|row| Ok(serde_json::from_str(&row.map_err(database_error)?)?)).collect()
        }).await
    }
    async fn events(&self, id: &str, after: u64, limit: usize) -> Result<Vec<RuntimeEvent>> {
        validate_id(id)?;
        if limit == 0 || limit > 4096 {
            return Err(Error::Invalid("limit must be between 1 and 4096".into()));
        }
        if after > i64::MAX as u64 {
            return Ok(vec![]);
        }
        let id = id.to_owned();
        self.database(move |connection| {
            let mut statement = connection.prepare("SELECT event_json FROM session_events WHERE session_id=?1 AND sequence>?2 ORDER BY sequence LIMIT ?3").map_err(database_error)?;
            let rows = statement.query_map(params![id, after as i64, limit as i64], |row| row.get::<_, String>(0)).map_err(database_error)?;
            rows.map(|row| Ok(serde_json::from_str(&row.map_err(database_error)?)?)).collect()
        }).await
    }
}
