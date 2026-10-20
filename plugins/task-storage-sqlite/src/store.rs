use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    fs::{File, OpenOptions, TryLockError},
    path::Path,
    time::Duration,
};

use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};
use swarmie_task_contracts::{
    CommitResult, MAX_PORTABLE_INTEGER, StorageOwner, TaskCursor, TaskLookup, TaskPage, TaskQuery,
    TaskRecord, TaskStatus, TaskTransaction, validate_identity,
};
use tembra::{RuntimeError, core::ErrorCode};

pub(crate) type Result<T> = std::result::Result<T, Error>;

type Metadata = (u64, String);

pub(crate) struct Store {
    connection: RefCell<Option<Connection>>,
    lock: RefCell<Option<File>>,
    owner: RefCell<Option<String>>,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error("SQLite storage failed: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("task storage filesystem access failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("stored task data is invalid: {0}")]
    Corrupt(String),
    #[error("task storage validation failed: {0}")]
    Invalid(String),
    #[error("task database is already owned")]
    Owned,
    #[error("the caller does not own task scheduling for this provider")]
    OwnerMismatch,
    #[error("the task storage Resource is closed")]
    Closed,
    #[error("unsupported task storage: {0}")]
    Unsupported(String),
    #[error("task scan cursor is stale or belongs to another query or provider")]
    Cursor,
}

impl Error {
    pub(crate) fn into_runtime(self) -> RuntimeError {
        let reason = match &self {
            Self::Database(_) | Self::Io(_) => "task_storage_io",
            Self::Corrupt(_) => "task_storage_corrupt",
            Self::Invalid(_) => "task_storage_invalid",
            Self::Owned => "task_storage_owned",
            Self::OwnerMismatch => "task_storage_owner_mismatch",
            Self::Closed => "task_storage_closed",
            Self::Unsupported(_) => "task_storage_unsupported",
            Self::Cursor => "task_storage_cursor",
        };
        let code = match &self {
            Self::Invalid(_) | Self::Cursor | Self::OwnerMismatch => ErrorCode::Invalid,
            Self::Unsupported(_) => ErrorCode::UpgradeRequired,
            Self::Closed => ErrorCode::Released,
            _ => ErrorCode::Unavailable,
        };
        RuntimeError::new(code, reason, self.to_string())
    }
}

impl Store {
    pub(crate) fn open(path: &Path) -> Result<Self> {
        // Materialize before canonicalization so aliases resolve to one stable lock path.
        let database_file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        reject_hard_links(&database_file)?;
        let database = path.canonicalize()?;
        let mut lock_name = database.as_os_str().to_owned();
        lock_name.push(".owner");
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(Path::new(&lock_name))?;
        match lock.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => return Err(Error::Owned),
            Err(TryLockError::Error(error)) => return Err(error.into()),
        }
        verify_lock_identity(&lock, Path::new(&lock_name))?;
        let mut connection = Connection::open_with_flags(
            &database,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        // Calls run on Tembra's local executor: refuse contention without blocking the Host.
        connection.busy_timeout(Duration::ZERO)?;
        let mode: String =
            connection.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))?;
        if !mode.eq_ignore_ascii_case("wal") {
            return Err(Error::Unsupported("SQLite refused WAL mode".into()));
        }
        connection.pragma_update(None, "synchronous", "FULL")?;
        let synchronization: i64 =
            connection.pragma_query_value(None, "synchronous", |row| row.get(0))?;
        if synchronization != 2 {
            return Err(Error::Unsupported(
                "SQLite refused FULL synchronization".into(),
            ));
        }
        crate::migrations::apply(&mut connection)?;
        Ok(Self {
            connection: RefCell::new(Some(connection)),
            lock: RefCell::new(Some(lock)),
            owner: RefCell::new(None),
        })
    }

    fn with<T>(&self, action: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let connection = self.connection.borrow();
        action(connection.as_ref().ok_or(Error::Closed)?)
    }

    pub(crate) fn claim_owner(&self, request: StorageOwner) -> Result<()> {
        identity(&request.id)?;
        self.with(|_| Ok(()))?;
        let mut owner = self.owner.borrow_mut();
        match owner.as_ref() {
            Some(current) if current != &request.id => Err(Error::Owned),
            Some(_) => Ok(()),
            None => {
                *owner = Some(request.id);
                Ok(())
            }
        }
    }

    pub(crate) fn release_owner(&self, request: StorageOwner) -> Result<()> {
        self.with(|_| Ok(()))?;
        let mut owner = self.owner.borrow_mut();
        if owner.as_ref() != Some(&request.id) {
            return Err(Error::OwnerMismatch);
        }
        *owner = None;
        Ok(())
    }

    pub(crate) fn get(&self, request: TaskLookup) -> Result<Option<TaskRecord>> {
        identity(&request.id)?;
        self.with(|connection| read_record(connection, &request.id))
    }

    pub(crate) fn scan(&self, query: TaskQuery) -> Result<TaskPage> {
        if !(1..=256).contains(&query.limit) {
            return Err(invalid("scan limit requires 1..=256"));
        }
        if query.statuses.len() > 8 {
            return Err(invalid("a status filter may name at most eight states"));
        }
        if let Some(parent) = &query.parent {
            identity(parent)?;
        }
        if let Some(cursor) = &query.cursor {
            identity(&cursor.after)?;
        }
        self.with(|connection| {
            // The provider is the sole writer and this synchronous method cannot interleave
            // another Contract call between metadata and record reads.
            let (sequence, store_id) = metadata(connection)?;
            if query.cursor.as_ref().is_some_and(|cursor| {
                cursor.sequence != sequence
                    || cursor.store_id != store_id
                    || cursor.statuses != query.statuses
                    || cursor.parent != query.parent
            }) {
                return Err(Error::Cursor);
            }
            let after = query
                .cursor
                .as_ref()
                .map_or("", |cursor| cursor.after.as_str());
            let mut sql = String::from(
                "SELECT id FROM task_records WHERE id > ?1 AND (?2 IS NULL OR parent = ?2)",
            );
            let mut values = vec![
                rusqlite::types::Value::Text(after.into()),
                query
                    .parent
                    .clone()
                    .map_or(rusqlite::types::Value::Null, rusqlite::types::Value::Text),
            ];
            if !query.statuses.is_empty() {
                sql.push_str(" AND status IN (");
                for (index, status) in query.statuses.iter().enumerate() {
                    if index > 0 {
                        sql.push(',');
                    }
                    sql.push('?');
                    values.push(rusqlite::types::Value::Text(status_name(*status).into()));
                }
                sql.push(')');
            }
            sql.push_str(" ORDER BY id COLLATE BINARY LIMIT ?");
            values.push(rusqlite::types::Value::Integer(i64::from(query.limit) + 1));
            let mut statement = connection.prepare(&sql)?;
            let ids = statement
                .query_map(rusqlite::params_from_iter(values), |row| {
                    row.get::<_, String>(0)
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            let mut records = Vec::with_capacity(ids.len());
            for id in ids {
                records.push(
                    read_record(connection, &id)?
                        .ok_or_else(|| Error::Corrupt("task disappeared during scan".into()))?,
                );
            }
            let next = if records.len() > query.limit as usize {
                records.pop();
                records.last().map(|record| TaskCursor {
                    store_id: store_id.clone(),
                    sequence,
                    after: record.id.clone(),
                    statuses: query.statuses.clone(),
                    parent: query.parent.clone(),
                })
            } else {
                None
            };
            Ok(TaskPage {
                sequence,
                records,
                next,
            })
        })
    }

    pub(crate) fn commit(&self, batch: TaskTransaction) -> Result<CommitResult> {
        if self.owner.borrow().as_deref() != Some(batch.owner.as_str()) {
            return Err(Error::OwnerMismatch);
        }
        if batch.writes.is_empty() || batch.writes.len() > 256 {
            return Err(invalid("transactions require 1..=256 task writes"));
        }
        let mut connection = self.connection.borrow_mut();
        let connection = connection.as_mut().ok_or(Error::Closed)?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (sequence, _) = metadata(&transaction)?;
        if sequence >= MAX_PORTABLE_INTEGER {
            return Err(invalid("commit sequence exhausted"));
        }
        let mut candidates = BTreeMap::new();
        let mut previous = BTreeMap::new();
        for write in &batch.writes {
            validate_record(&write.record)?;
            let record = &write.record;
            if candidates
                .insert(record.id.clone(), record.clone())
                .is_some()
            {
                return Err(invalid("duplicate task write"));
            }
            let existing = read_record(&transaction, &record.id)?;
            let actual_revision = existing.as_ref().map(|record| record.revision);
            if actual_revision != write.expected_revision {
                return Ok(CommitResult::Conflict {
                    id: record.id.clone(),
                    actual_revision,
                });
            }
            let required = write
                .expected_revision
                .unwrap_or(0)
                .checked_add(1)
                .ok_or_else(|| invalid("revision exhausted"))?;
            if record.revision != required {
                return Err(invalid(
                    "replacement revision must equal expected revision plus one",
                ));
            }
            if let Some(existing) = &existing {
                if existing.is_terminal() {
                    return Err(invalid("terminal task receipts are immutable"));
                }
                if existing.kind != record.kind
                    || existing.parent != record.parent
                    || existing.input != record.input
                {
                    return Err(invalid("task kind, parent and input are immutable"));
                }
                if existing.cancellation_requested && !record.cancellation_requested {
                    return Err(invalid("cancellation intent is monotone"));
                }
                if existing.status == TaskStatus::Completing
                    && (existing.outcome != record.outcome
                        || !matches!(record.status, TaskStatus::Completing | TaskStatus::Terminal))
                {
                    return Err(invalid("a completing task retains its decided outcome"));
                }
            }
            previous.insert(record.id.clone(), existing);
        }
        validate_relationships(&transaction, &candidates, &previous)?;
        for record in candidates.values() {
            let portable = tembra::contracts::to_portable_value(record)
                .map_err(|error| invalid(error.to_string()))?;
            let encoded =
                serde_json::to_string(&portable).map_err(|error| invalid(error.to_string()))?;
            transaction.execute(
                "INSERT INTO task_records(id, revision, status, parent, record) VALUES(?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(id) DO UPDATE SET revision = excluded.revision, status = excluded.status, parent = excluded.parent, record = excluded.record",
                params![record.id, record.revision, status_name(record.status), record.parent, encoded],
            )?;
        }
        let sequence = sequence + 1;
        transaction.execute(
            "UPDATE task_storage_metadata SET sequence = ?1 WHERE singleton = 1",
            [sequence],
        )?;
        transaction.commit()?;
        Ok(CommitResult::Committed {
            sequence,
            records: batch.writes.into_iter().map(|write| write.record).collect(),
        })
    }

    pub(crate) fn close(&self) -> Result<()> {
        let connection = self.connection.borrow_mut().take();
        if let Some(connection) = connection {
            // Release the OS lock only after SQLite has closed successfully.
            if let Err((connection, error)) = connection.close() {
                *self.connection.borrow_mut() = Some(connection);
                return Err(error.into());
            }
        }
        *self.owner.borrow_mut() = None;
        self.lock.borrow_mut().take();
        Ok(())
    }
}

fn invalid(message: impl Into<String>) -> Error {
    Error::Invalid(message.into())
}

fn identity(value: &str) -> Result<()> {
    validate_identity(value).map_err(|error| invalid(error.to_string()))
}

fn metadata(connection: &Connection) -> Result<Metadata> {
    connection
        .query_row(
            "SELECT sequence, store_id FROM task_storage_metadata WHERE singleton = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(Error::from)
}

fn decode(encoded: &str) -> Result<TaskRecord> {
    // Validate duplicate keys, safe numbers and depth without changing Serde's numeric
    // representation: the portable parser canonicalizes integral float tokens.
    tembra::contracts::parse_json(encoded).map_err(|error| Error::Corrupt(error.to_string()))?;
    let record: TaskRecord =
        serde_json::from_str(encoded).map_err(|error| Error::Corrupt(error.to_string()))?;
    validate_record(&record).map_err(|error| Error::Corrupt(error.to_string()))?;
    Ok(record)
}

fn read_record(connection: &Connection, id: &str) -> Result<Option<TaskRecord>> {
    let row: Option<(String, u64, String, Option<String>)> = connection
        .query_row(
            "SELECT record, revision, status, parent FROM task_records WHERE id = ?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?;
    row.map(|(encoded, revision, status, parent)| {
        let record = decode(&encoded)?;
        if record.id != id
            || record.revision != revision
            || status_name(record.status) != status
            || record.parent != parent
        {
            return Err(Error::Corrupt(
                "task record and indexed identity disagree".into(),
            ));
        }
        Ok(record)
    })
    .transpose()
}

fn validate_record(record: &TaskRecord) -> Result<()> {
    tembra::contracts::to_portable_value(record).map_err(|error| invalid(error.to_string()))?;
    identity(&record.id)?;
    identity(&record.kind)?;
    identity(&record.phase)?;
    if record.definition_version == 0
        || record.revision == 0
        || record.revision > MAX_PORTABLE_INTEGER
    {
        return Err(invalid(
            "definition version and revision must be positive portable integers",
        ));
    }
    if record
        .wake_at_ms
        .is_some_and(|wake| wake > MAX_PORTABLE_INTEGER)
    {
        return Err(invalid("wake instant exceeds portable integer range"));
    }
    if let Some(parent) = &record.parent {
        identity(parent)?;
    }
    if record.wait_for.len() > 256 {
        return Err(invalid("a wait may name at most 256 direct children"));
    }
    let mut waits = BTreeSet::new();
    for id in &record.wait_for {
        identity(id)?;
        if !waits.insert(id) {
            return Err(invalid("wait children must be distinct"));
        }
    }
    if (record.status == TaskStatus::Waiting) != !record.wait_for.is_empty() {
        return Err(invalid("only waiting tasks have a nonempty wait list"));
    }
    if (record.status == TaskStatus::Sleeping) != record.wake_at_ms.is_some() {
        return Err(invalid("only sleeping tasks have a wake instant"));
    }
    if matches!(record.status, TaskStatus::Completing | TaskStatus::Terminal)
        != record.outcome.is_some()
    {
        return Err(invalid("only completing or terminal tasks have an outcome"));
    }
    Ok(())
}

fn candidate(
    connection: &Connection,
    candidates: &BTreeMap<String, TaskRecord>,
    id: &str,
) -> Result<Option<TaskRecord>> {
    match candidates.get(id) {
        Some(record) => Ok(Some(record.clone())),
        None => read_record(connection, id),
    }
}

fn validate_relationships(
    connection: &Connection,
    candidates: &BTreeMap<String, TaskRecord>,
    previous: &BTreeMap<String, Option<TaskRecord>>,
) -> Result<()> {
    for record in candidates.values() {
        if let Some(parent) = &record.parent {
            let owner = candidate(connection, candidates, parent)?
                .ok_or_else(|| invalid("task parent does not exist"))?;
            if previous.get(&record.id).is_some_and(Option::is_none)
                && (owner.cancellation_requested
                    || matches!(owner.status, TaskStatus::Completing | TaskStatus::Terminal))
            {
                return Err(invalid(
                    "new children require a live parent without cancellation intent",
                ));
            }
            let mut visited = BTreeSet::from([record.id.clone()]);
            let mut current = Some(owner);
            while let Some(ancestor) = current {
                if !visited.insert(ancestor.id.clone()) {
                    return Err(invalid("task ownership contains a cycle"));
                }
                current = match ancestor.parent.as_deref() {
                    Some(id) => Some(candidate(connection, candidates, id)?.ok_or_else(|| {
                        Error::Corrupt("an ownership ancestor does not exist".into())
                    })?),
                    None => None,
                };
            }
        }
        for id in &record.wait_for {
            let child = candidate(connection, candidates, id)?
                .ok_or_else(|| invalid("wait target does not exist"))?;
            if child.parent.as_deref() != Some(record.id.as_str()) {
                return Err(invalid("durable waits may name direct owned children only"));
            }
        }
        if record.is_terminal() {
            let mut children = BTreeMap::new();
            let mut statement =
                connection.prepare("SELECT record FROM task_records WHERE parent = ?1")?;
            let mut rows = statement.query([&record.id])?;
            while let Some(row) = rows.next()? {
                let child = decode(&row.get::<_, String>(0)?)?;
                children.insert(child.id.clone(), child);
            }
            for child in candidates
                .values()
                .filter(|child| child.parent.as_deref() == Some(record.id.as_str()))
            {
                children.insert(child.id.clone(), child.clone());
            }
            if children.values().any(|child| !child.is_terminal()) {
                return Err(invalid(
                    "terminal completion requires all owned children to settle",
                ));
            }
        }
    }
    Ok(())
}

fn status_name(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Pending => "pending",
        TaskStatus::Running => "running",
        TaskStatus::Waiting => "waiting",
        TaskStatus::Sleeping => "sleeping",
        TaskStatus::Completing => "completing",
        TaskStatus::Blocked => "blocked",
        TaskStatus::Parked => "parked",
        TaskStatus::Terminal => "terminal",
    }
}

#[cfg(unix)]
fn reject_hard_links(file: &File) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    if file.metadata()?.nlink() != 1 {
        return Err(Error::Unsupported(
            "hard-linked database files are not supported".into(),
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn reject_hard_links(_file: &File) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn verify_lock_identity(lock: &File, path: &Path) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    let held = lock.metadata()?;
    let named = path.metadata()?;
    if held.ino() != named.ino() || held.dev() != named.dev() {
        return Err(Error::Owned);
    }
    Ok(())
}

#[cfg(not(unix))]
fn verify_lock_identity(_lock: &File, _path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use swarmie_task_contracts::{TaskSpec, TaskWrite};

    fn pending(id: &str) -> TaskWrite {
        TaskWrite {
            expected_revision: None,
            record: TaskRecord::new(
                TaskSpec {
                    id: id.into(),
                    kind: "test.storage".into(),
                    definition_version: 1,
                    input: serde_json::json!(null),
                    checkpoint: serde_json::json!({"phase":"ready"}),
                    phase: "ready".into(),
                },
                None,
            ),
        }
    }

    #[test]
    fn sqlite_failure_rolls_back_prior_writes_and_sequence() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let database = directory.path().join("tasks.sqlite");
        let store = Store::open(&database)?;
        store.claim_owner(StorageOwner {
            id: "unit-owner".into(),
        })?;
        let raw = Connection::open(&database)?;
        raw.execute_batch("CREATE TRIGGER reject_fixture BEFORE INSERT ON task_records WHEN NEW.id = 'z-rejected' BEGIN SELECT RAISE(FAIL, 'injected write failure'); END;")?;
        let before = store.scan(TaskQuery::default())?;
        assert!(matches!(
            store.commit(TaskTransaction {
                owner: "unit-owner".into(),
                writes: vec![pending("a-first"), pending("z-rejected")]
            }),
            Err(Error::Database(_))
        ));
        assert_eq!(store.scan(TaskQuery::default())?, before);
        store.close()?;
        assert!(matches!(
            store.get(TaskLookup {
                id: "a-first".into()
            }),
            Err(Error::Closed)
        ));
        store.close()?;
        let reopened = Store::open(&database)?;
        assert_eq!(reopened.scan(TaskQuery::default())?, before);
        Ok(())
    }

    #[test]
    fn nonportable_native_values_are_refused_before_persistence() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let store = Store::open(&directory.path().join("tasks.sqlite"))?;
        store.claim_owner(StorageOwner {
            id: "unit-owner".into(),
        })?;
        let mut write = pending("unsafe-number");
        write.record.checkpoint = serde_json::json!({"nested":[MAX_PORTABLE_INTEGER + 1]});
        assert!(matches!(
            store.commit(TaskTransaction {
                owner: "unit-owner".into(),
                writes: vec![write]
            }),
            Err(Error::Invalid(_))
        ));
        assert_eq!(store.scan(TaskQuery::default())?.sequence, 0);
        assert!(
            store
                .get(TaskLookup {
                    id: "unsafe-number".into()
                })?
                .is_none()
        );
        Ok(())
    }

    #[test]
    fn owner_admission_cas_and_snapshot_invalidation_are_independent() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let store = Store::open(&directory.path().join("tasks.sqlite"))?;
        store.claim_owner(StorageOwner {
            id: "unit-owner".into(),
        })?;
        assert!(matches!(
            store.claim_owner(StorageOwner {
                id: "competing-owner".into()
            }),
            Err(Error::Owned)
        ));
        assert!(matches!(
            store.commit(TaskTransaction {
                owner: "unit-owner".into(),
                writes: vec![pending("a"), pending("b")]
            })?,
            CommitResult::Committed { sequence: 1, .. }
        ));
        let first = store.scan(TaskQuery {
            limit: 1,
            ..Default::default()
        })?;
        let mut write = pending("a");
        write.expected_revision = Some(1);
        write.record.revision = 2;
        write.record.phase = "next".into();
        store.commit(TaskTransaction {
            owner: "unit-owner".into(),
            writes: vec![write.clone()],
        })?;
        assert!(matches!(
            store.commit(TaskTransaction {
                owner: "unit-owner".into(),
                writes: vec![write]
            })?,
            CommitResult::Conflict {
                actual_revision: Some(2),
                ..
            }
        ));
        assert!(matches!(
            store.scan(TaskQuery {
                cursor: first.next,
                ..Default::default()
            }),
            Err(Error::Cursor)
        ));
        Ok(())
    }
}
