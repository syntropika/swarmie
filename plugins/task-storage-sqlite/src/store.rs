use std::{
    borrow::Cow,
    cell::{Cell, RefCell},
    collections::{BTreeMap, BTreeSet},
    fs::{File, OpenOptions, TryLockError},
    path::Path,
    time::Duration,
};

use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use swarmie_task_contracts::{
    CommitResult, EncodedTaskRecord, MAX_PORTABLE_INTEGER, MAX_TASK_PAGE_BYTES,
    MAX_TASK_RECORD_BYTES, StorageOwner, TaskCursor, TaskLookup, TaskPage, TaskQuery, TaskRecord,
    TaskStatus, TaskStorageIdentity, TaskTransaction, committed_receipt_bytes,
    encode_task_transaction, task_wire_bytes, validate_identity, validate_task_payload_bytes,
    validate_task_record_encoding,
};
use tembra::{RuntimeError, core::ErrorCode};

pub(crate) type Result<T> = std::result::Result<T, Error>;

/// Holds every fixed and transaction-control statement plus the eighteen filtered scan shapes.
const STATEMENT_CACHE_CAPACITY: usize = 64;

/// A transaction whose control statements come from the connection's statement cache.
///
/// It keeps the rollback semantics of rusqlite's default guard: dropping it without a successful
/// COMMIT rolls back, including after a COMMIT that failed while SQLite still holds the
/// transaction. Statements borrowing it always end before it drops.
pub(crate) struct Transaction<'c> {
    connection: &'c Connection,
}

impl<'c> Transaction<'c> {
    /// Takes the database write lock immediately; contention fails at once (zero busy timeout).
    pub(crate) fn immediate(connection: &'c Connection) -> rusqlite::Result<Self> {
        control(connection, "BEGIN IMMEDIATE")?;
        Ok(Self { connection })
    }

    /// Reads one consistent snapshot without taking the write lock.
    pub(crate) fn deferred(connection: &'c Connection) -> rusqlite::Result<Self> {
        control(connection, "BEGIN DEFERRED")?;
        Ok(Self { connection })
    }

    pub(crate) fn commit(self) -> rusqlite::Result<()> {
        control(self.connection, "COMMIT")
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
    pub(crate) connection: RefCell<Option<Connection>>,
    pub(crate) outbox_limits: crate::TaskOutboxLimits,
    /// Immutable database identity, read while opening. No operation updates it.
    pub(crate) store_id: String,
    /// Last committed sequence. Only this provider's commits advance it while the ownership
    /// lock is held. `None` follows a failed COMMIT whose outcome could not be re-read.
    sequence: Cell<Option<u64>>,
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
    #[error("task storage size limit exceeded: {0}")]
    TooLarge(String),
    #[error("task outbox retained capacity or consumer capacity is exhausted")]
    OutboxFull,
    #[error("the caller does not own this outbox consumer")]
    OutboxOwnerMismatch,
    #[error("this legacy outbox consumer requires explicit operator ownership adoption")]
    OutboxOwnerUnassigned,
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
            Self::TooLarge(_) => "task_storage_too_large",
            Self::OutboxFull => "task_storage_outbox_full",
            Self::OutboxOwnerMismatch => "task_storage_outbox_owner_mismatch",
            Self::OutboxOwnerUnassigned => "task_storage_outbox_owner_unassigned",
            Self::Owned => "task_storage_owned",
            Self::OwnerMismatch => "task_storage_owner_mismatch",
            Self::Closed => "task_storage_closed",
            Self::Unsupported(_) => "task_storage_unsupported",
            Self::Cursor => "task_storage_cursor",
        };
        let code = match &self {
            Self::Invalid(_) | Self::TooLarge(_) | Self::Cursor | Self::OwnerMismatch => {
                ErrorCode::Invalid
            }
            Self::OutboxOwnerMismatch => ErrorCode::Forbidden,
            Self::Unsupported(_) | Self::OutboxOwnerUnassigned => ErrorCode::UpgradeRequired,
            Self::Closed => ErrorCode::Released,
            _ => ErrorCode::Unavailable,
        };
        RuntimeError::new(code, reason, self.to_string())
    }
}

impl Store {
    #[cfg(test)]
    pub(crate) fn open(path: &Path) -> Result<Self> {
        Self::open_with_outbox_limits(path, crate::TaskOutboxLimits::default(), &BTreeMap::new())
    }

    pub(crate) fn open_with_outbox_limits(
        path: &Path,
        outbox_limits: crate::TaskOutboxLimits,
        legacy_owners: &BTreeMap<String, String>,
    ) -> Result<Self> {
        outbox_limits
            .validate()
            .map_err(|error| invalid(error.to_string()))?;
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
        let (sequence, store_id) = crate::migrations::apply(&mut connection, legacy_owners)?;
        connection.set_prepared_statement_cache_capacity(STATEMENT_CACHE_CAPACITY);
        Ok(Self {
            connection: RefCell::new(Some(connection)),
            outbox_limits,
            store_id,
            sequence: Cell::new(Some(sequence)),
            lock: RefCell::new(Some(lock)),
            owner: RefCell::new(None),
        })
    }

    pub(crate) fn with<T>(&self, action: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let connection = self.connection.borrow();
        action(connection.as_ref().ok_or(Error::Closed)?)
    }

    /// Answers from the identity read while opening, without SQL, ownership or capacity checks.
    pub(crate) fn store_identity(&self) -> Result<TaskStorageIdentity> {
        self.with(|_| {
            Ok(TaskStorageIdentity {
                store_id: self.store_id.clone(),
            })
        })
    }

    /// Returns the committed sequence. SQLite is consulted only after an uncertain COMMIT.
    fn committed_sequence(&self, connection: &Connection) -> Result<u64> {
        if let Some(sequence) = self.sequence.get() {
            return Ok(sequence);
        }
        let sequence = read_sequence(connection)?;
        self.sequence.set(Some(sequence));
        Ok(sequence)
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
        // A readable record never exceeds MAX_TASK_RECORD_BYTES, which is below the page budget,
        // and `Some(record)` encodes exactly as the record. No second measurement is needed.
        self.with(|connection| Ok(read_record(connection, &request.id)?.map(|(record, _)| record)))
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
            // another Contract call between the committed sequence and record reads.
            let sequence = self.committed_sequence(connection)?;
            if query.cursor.as_ref().is_some_and(|cursor| {
                cursor.sequence != sequence
                    || cursor.store_id != self.store_id
                    || cursor.statuses != query.statuses
                    || cursor.parent != query.parent
            }) {
                return Err(Error::Cursor);
            }
            let after = query
                .cursor
                .as_ref()
                .map_or("", |cursor| cursor.after.as_str());
            let (sql, values) = scan_sql(&query, after);
            let ids = connection
                .prepare_cached(&sql)?
                .query_map(rusqlite::params_from_iter(values), |row| {
                    row.get::<_, String>(0)
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            let cursor = |after: String| TaskCursor {
                store_id: self.store_id.clone(),
                sequence,
                after,
                statuses: query.statuses.clone(),
                parent: query.parent.clone(),
            };
            let mut page = TaskPage {
                sequence,
                records: vec![],
                next: None,
            };
            // Measure both small envelopes once. A cursor naming a validated identity adds
            // exactly that many unescaped bytes to the envelope measured with an empty one.
            let (final_envelope, cursor_envelope) = if ids.is_empty() {
                (0, 0)
            } else {
                (
                    task_wire_bytes(&page).map_err(size_error)?,
                    task_wire_bytes(&TaskPage {
                        sequence,
                        records: vec![],
                        next: Some(cursor(String::new())),
                    })
                    .map_err(size_error)?,
                )
            };
            let mut records_bytes = 0;
            for (index, id) in ids.iter().enumerate() {
                if page.records.len() == query.limit as usize {
                    break;
                }
                // The stored canonical text length is the record's exact wire size.
                let (record, record_bytes) = read_record(connection, id)?
                    .ok_or_else(|| Error::Corrupt("task disappeared during scan".into()))?;
                let next = (index + 1 < ids.len()).then(|| cursor(id.clone()));
                let envelope_bytes = if next.is_some() {
                    cursor_envelope + id.len()
                } else {
                    final_envelope
                };
                // An empty records array already contributes its brackets. Add each
                // canonical record and exactly one comma per prior element.
                let candidate_bytes = envelope_bytes
                    + records_bytes
                    + record_bytes
                    + usize::from(!page.records.is_empty());
                if candidate_bytes > MAX_TASK_PAGE_BYTES {
                    if page.records.is_empty() {
                        return Err(Error::TooLarge("one record exceeds the page budget".into()));
                    }
                    // The prior candidate's cursor already names the last included row.
                    // This additional row proves that continuation remains necessary.
                    break;
                }
                records_bytes += record_bytes + usize::from(!page.records.is_empty());
                page.records.push(record);
                page.next = next;
            }
            Ok(page)
        })
    }

    pub(crate) fn commit(&self, batch: TaskTransaction) -> Result<CommitResult> {
        if self.owner.borrow().as_deref() != Some(batch.owner.as_str()) {
            return Err(Error::OwnerMismatch);
        }
        if batch.writes.is_empty() || batch.writes.len() > 256 {
            return Err(invalid("transactions require 1..=256 task writes"));
        }
        // Encode each record exactly once. That canonical text is the stored row and the exact
        // basis of the record, control reserve, request and receipt budgets; the reserve comes
        // from the same member encodings, so a large outcome is never encoded again. Arguments
        // reach this provider only through Tembra's portable decoding, which already
        // represents every integral number as an integer, so immutable comparisons below
        // observe canonical portable numbers.
        let encoded = encode_task_transaction(&batch).map_err(size_error)?;
        validate_task_payload_bytes(encoded.bytes).map_err(size_error)?;
        let connection = self.connection.borrow();
        let connection = connection.as_ref().ok_or(Error::Closed)?;
        let transaction = Transaction::immediate(connection)?;
        let sequence = self.committed_sequence(&transaction)?;
        if sequence >= MAX_PORTABLE_INTEGER {
            return Err(invalid("commit sequence exhausted"));
        }
        let mut candidates = BTreeMap::new();
        let mut previous = BTreeMap::new();
        for (index, write) in batch.writes.iter().enumerate() {
            validate_record(&write.record, &encoded.records[index])?;
            let record = &write.record;
            if candidates.insert(record.id.as_str(), index).is_some() {
                return Err(invalid("duplicate task write"));
            }
            let existing = read_record(&transaction, &record.id)?.map(|(record, _)| record);
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
        validate_relationships(&transaction, &batch, &candidates, &previous)?;
        validate_task_payload_bytes(
            committed_receipt_bytes(
                sequence + 1,
                encoded.records.iter().map(|record| record.text().len()),
            )
            .map_err(size_error)?,
        )
        .map_err(size_error)?;
        let events = crate::outbox::prepare_events(
            &transaction,
            &batch,
            &previous,
            &self.store_id,
            sequence + 1,
            self.outbox_limits,
        )?;
        {
            let mut upsert = transaction.prepare_cached(
                "INSERT INTO task_records(id, revision, status, parent, record) VALUES(?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(id) DO UPDATE SET revision = excluded.revision, status = excluded.status, parent = excluded.parent, record = excluded.record",
            )?;
            for &index in candidates.values() {
                let record = &batch.writes[index].record;
                upsert.execute(params![
                    record.id,
                    record.revision,
                    status_name(record.status),
                    record.parent,
                    encoded.records[index].text()
                ])?;
            }
        }
        crate::outbox::append_events(&transaction, &events)?;
        let sequence = sequence + 1;
        transaction
            .prepare_cached("UPDATE task_storage_metadata SET sequence = ?1 WHERE singleton = 1")?
            .execute([sequence])?;
        if let Err(error) = transaction.commit() {
            // A failed COMMIT has an unknown outcome. Adopt what SQLite now reports, or force
            // the next caller to read it again when even that read fails.
            self.sequence.set(read_sequence(connection).ok());
            return Err(error.into());
        }
        self.sequence.set(Some(sequence));
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

fn scan_sql(query: &TaskQuery, after: &str) -> (String, Vec<rusqlite::types::Value>) {
    // Explicit index selection keeps retained terminal history outside active scans even
    // when seven status ranges plus ORDER BY would make SQLite prefer the primary key.
    let index = if query.parent.is_some() {
        " INDEXED BY task_records_parent"
    } else if !query.statuses.is_empty() {
        " INDEXED BY task_records_status"
    } else {
        ""
    };
    let mut sql = format!("SELECT id FROM task_records{index} WHERE id > ?");
    let mut values = vec![rusqlite::types::Value::Text(after.into())];
    if let Some(parent) = &query.parent {
        sql.push_str(" AND parent = ?");
        values.push(rusqlite::types::Value::Text(parent.clone()));
    }
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
    (sql, values)
}

fn invalid(message: impl Into<String>) -> Error {
    Error::Invalid(message.into())
}

fn size_error(error: RuntimeError) -> Error {
    if error.reason == "task_storage_too_large" {
        Error::TooLarge(error.message)
    } else {
        Error::Invalid(error.to_string())
    }
}

fn identity(value: &str) -> Result<()> {
    validate_identity(value).map_err(|error| invalid(error.to_string()))
}

fn read_sequence(connection: &Connection) -> Result<u64> {
    Ok(connection
        .prepare_cached("SELECT sequence FROM task_storage_metadata WHERE singleton = 1")?
        .query_row([], |row| row.get(0))?)
}

/// Decodes a row that this provider admitted and stored as canonical text. That text length is
/// the record's exact wire size, so only the record ceiling is checked. One typed parse refuses
/// malformed JSON, duplicate, unknown or mistyped fields, and the structural checks refuse
/// impossible records. Refused rows remain stored for explicit operator repair.
///
/// The parse keeps serde_json's default recursion limit, which accepts 127 nested containers
/// and bounds recursion on tampered rows. Admission keeps every stored record within it: a
/// transaction nests each record within three containers of Tembra's 128, so a stored record
/// holds at most 125.
fn decode(encoded: &str) -> Result<TaskRecord> {
    if encoded.len() > MAX_TASK_RECORD_BYTES {
        return Err(Error::TooLarge(format!(
            "stored task record contains {} bytes; maximum is {MAX_TASK_RECORD_BYTES}",
            encoded.len()
        )));
    }
    let record: TaskRecord =
        serde_json::from_str(encoded).map_err(|error| Error::Corrupt(error.to_string()))?;
    check_record(&record).map_err(|error| Error::Corrupt(error.to_string()))?;
    Ok(record)
}

/// Reads one record and its canonical stored length, cross-checking the indexed columns.
fn read_record(connection: &Connection, id: &str) -> Result<Option<(TaskRecord, usize)>> {
    let row: Option<(String, u64, String, Option<String>)> = connection
        .prepare_cached("SELECT record, revision, status, parent FROM task_records WHERE id = ?1")?
        .query_row([id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
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
        Ok((record, encoded.len()))
    })
    .transpose()
}

/// Admits a replacement record from its single canonical encoding.
fn validate_record(record: &TaskRecord, encoded: &EncodedTaskRecord) -> Result<()> {
    validate_task_record_encoding(encoded).map_err(size_error)?;
    check_record(record)
}

/// Structural record rules shared by admission and trusted reads.
fn check_record(record: &TaskRecord) -> Result<()> {
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

/// Prefers the transaction's replacement, borrowed without copying, over the stored record.
fn candidate<'a>(
    connection: &Connection,
    batch: &'a TaskTransaction,
    candidates: &BTreeMap<&str, usize>,
    id: &str,
) -> Result<Option<Cow<'a, TaskRecord>>> {
    match candidates.get(id) {
        Some(&index) => Ok(Some(Cow::Borrowed(&batch.writes[index].record))),
        None => Ok(read_record(connection, id)?.map(|(record, _)| Cow::Owned(record))),
    }
}

fn validate_relationships(
    connection: &Connection,
    batch: &TaskTransaction,
    candidates: &BTreeMap<&str, usize>,
    previous: &BTreeMap<String, Option<TaskRecord>>,
) -> Result<()> {
    for &index in candidates.values() {
        let record = &batch.writes[index].record;
        if let Some(parent) = &record.parent {
            let owner = candidate(connection, batch, candidates, parent)?
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
                    Some(id) => Some(candidate(connection, batch, candidates, id)?.ok_or_else(
                        || Error::Corrupt("an ownership ancestor does not exist".into()),
                    )?),
                    None => None,
                };
            }
        }
        for id in &record.wait_for {
            let child = candidate(connection, batch, candidates, id)?
                .ok_or_else(|| invalid("wait target does not exist"))?;
            if child.parent.as_deref() != Some(record.id.as_str()) {
                return Err(invalid("durable waits may name direct owned children only"));
            }
        }
        if record.is_terminal() {
            let mut children = BTreeMap::new();
            let mut statement =
                connection.prepare_cached("SELECT record FROM task_records WHERE parent = ?1")?;
            let mut rows = statement.query([&record.id])?;
            while let Some(row) = rows.next()? {
                let child = decode(&row.get::<_, String>(0)?)?;
                children.insert(child.id.clone(), child.is_terminal());
            }
            for child in candidates
                .values()
                .map(|&index| &batch.writes[index].record)
                .filter(|child| child.parent.as_deref() == Some(record.id.as_str()))
            {
                children.insert(child.id.clone(), child.is_terminal());
            }
            if children.values().any(|terminal| !terminal) {
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
            provenance: None,
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
    fn filtered_scans_use_relationship_and_active_status_indices() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let store = Store::open(&directory.path().join("tasks.sqlite"))?;
        let active = vec![
            TaskStatus::Pending,
            TaskStatus::Running,
            TaskStatus::Waiting,
            TaskStatus::Sleeping,
            TaskStatus::Completing,
            TaskStatus::Blocked,
            TaskStatus::Parked,
        ];
        for (query, index) in [
            (
                TaskQuery {
                    statuses: active.clone(),
                    ..Default::default()
                },
                "task_records_status",
            ),
            (
                TaskQuery {
                    parent: Some("parent".into()),
                    ..Default::default()
                },
                "task_records_parent",
            ),
            (
                TaskQuery {
                    parent: Some("parent".into()),
                    statuses: active,
                    ..Default::default()
                },
                "task_records_parent",
            ),
        ] {
            let (sql, values) = scan_sql(&query, "cursor-id");
            let plan = store.with(|connection| {
                let mut statement = connection.prepare(&format!("EXPLAIN QUERY PLAN {sql}"))?;
                let rows = statement.query_map(rusqlite::params_from_iter(values), |row| {
                    row.get::<_, String>(3)
                })?;
                Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
            })?;
            assert!(plan.iter().any(|line| line.contains(index)), "{plan:?}");
            assert!(
                !plan.iter().any(|line| line.contains("PRIMARY KEY")),
                "{plan:?}"
            );
        }
        Ok(())
    }

    #[test]
    fn legacy_oversized_record_is_explicitly_refused_without_deletion() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let store = Store::open(&directory.path().join("tasks.sqlite"))?;
        let mut record = pending("legacy").record;
        record.checkpoint = serde_json::json!("x".repeat(800_000));
        let encoded = serde_json::to_string(&record).expect("fixture encoding");
        store.with(|connection| {
            connection.execute("INSERT INTO task_records(id, revision, status, parent, record) VALUES('legacy', 1, 'pending', NULL, ?1)", [&encoded])?;
            Ok(())
        })?;
        assert!(matches!(
            store.get(TaskLookup {
                id: "legacy".into()
            }),
            Err(Error::TooLarge(_))
        ));
        assert!(matches!(
            store.scan(TaskQuery::default()),
            Err(Error::TooLarge(_))
        ));
        store.with(|connection| {
            assert_eq!(
                connection.query_row(
                    "SELECT record FROM task_records WHERE id='legacy'",
                    [],
                    |row| row.get::<_, String>(0)
                )?,
                encoded
            );
            Ok(())
        })
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
        assert_eq!(
            store.sequence.get(),
            Some(before.sequence),
            "a statement failure before COMMIT keeps the cached sequence"
        );
        let identity = store.store_identity()?;
        store.close()?;
        assert!(matches!(store.store_identity(), Err(Error::Closed)));
        assert!(matches!(
            store.get(TaskLookup {
                id: "a-first".into()
            }),
            Err(Error::Closed)
        ));
        store.close()?;
        let reopened = Store::open(&database)?;
        assert_eq!(reopened.scan(TaskQuery::default())?, before);
        assert_eq!(reopened.store_identity()?, identity);
        Ok(())
    }

    #[test]
    fn cached_metadata_matches_sqlite_and_is_reread_after_an_uncertain_commit() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let database = directory.path().join("tasks.sqlite");
        let store = Store::open(&database)?;
        let raw = Connection::open(&database)?;
        let (sequence, store_id): (u64, String) = raw.query_row(
            "SELECT sequence, store_id FROM task_storage_metadata WHERE singleton = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        assert_eq!(store.store_identity()?.store_id, store_id);
        assert_eq!(store.sequence.get(), Some(sequence));
        // Identity needs neither scheduler ownership nor a database read.
        store.claim_owner(StorageOwner {
            id: "unit-owner".into(),
        })?;
        assert!(matches!(
            store.commit(TaskTransaction {
                owner: "unit-owner".into(),
                writes: vec![pending("a")]
            })?,
            CommitResult::Committed { sequence: 1, .. }
        ));
        assert_eq!(store.sequence.get(), Some(1));
        // A failed COMMIT leaves an unknown outcome and forgets the cached sequence; the next
        // call adopts whatever SQLite reports before validating cursors or committing.
        store.sequence.set(None);
        let page = store.scan(TaskQuery {
            limit: 1,
            ..Default::default()
        })?;
        assert_eq!(page.sequence, 1);
        assert_eq!(store.sequence.get(), Some(1));
        store.sequence.set(None);
        assert!(matches!(
            store.commit(TaskTransaction {
                owner: "unit-owner".into(),
                writes: vec![pending("b")]
            })?,
            CommitResult::Committed { sequence: 2, .. }
        ));
        assert_eq!(store.sequence.get(), Some(2));
        let persisted: u64 = raw.query_row(
            "SELECT sequence FROM task_storage_metadata WHERE singleton = 1",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(persisted, 2);
        assert_eq!(store.store_identity()?.store_id, store_id);
        Ok(())
    }

    #[test]
    fn a_failed_commit_rolls_back_before_the_sequence_is_reread() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let store = Store::open(&directory.path().join("tasks.sqlite"))?;
        store.claim_owner(StorageOwner {
            id: "unit-owner".into(),
        })?;
        let commit = |id: &str| {
            store.commit(TaskTransaction {
                owner: "unit-owner".into(),
                writes: vec![pending(id)],
            })
        };
        assert!(matches!(
            commit("a")?,
            CommitResult::Committed { sequence: 1, .. }
        ));
        // A deferred foreign key violation fails COMMIT itself. SQLite then keeps the
        // transaction open, so a sequence read before the guard rolls back would observe the
        // uncommitted value.
        store.with(|connection| {
            connection.execute_batch(
                "PRAGMA foreign_keys = ON;
                 CREATE TABLE fk_parent(id INTEGER PRIMARY KEY);
                 CREATE TABLE fk_child(parent INTEGER REFERENCES fk_parent(id) DEFERRABLE INITIALLY DEFERRED);
                 CREATE TRIGGER fk_commit_failure AFTER INSERT ON task_records WHEN NEW.id = 'z-commit'
                 BEGIN INSERT INTO fk_child VALUES(42); END;",
            )?;
            Ok(())
        })?;
        let durable = || {
            store.with(|connection| {
                Ok(connection.query_row(
                    "SELECT (SELECT sequence FROM task_storage_metadata WHERE singleton = 1),
                            (SELECT count(*) FROM task_outbox),
                            (SELECT retained_events FROM task_outbox_metadata WHERE singleton = 1)",
                    [],
                    |row| {
                        Ok((
                            row.get::<_, u64>(0)?,
                            row.get::<_, u64>(1)?,
                            row.get::<_, u64>(2)?,
                        ))
                    },
                )?)
            })
        };
        let before = durable()?;
        assert_eq!(before.0, 1);
        match commit("z-commit") {
            Err(Error::Database(error)) => {
                assert!(error.to_string().contains("FOREIGN KEY"), "{error}");
            }
            other => panic!("COMMIT must fail on the deferred violation: {other:?}"),
        }
        assert!(store.with(|connection| Ok(connection.is_autocommit()))?);
        assert_eq!(
            store.sequence.get(),
            Some(1),
            "the cache adopts the sequence that survived the rollback"
        );
        assert_eq!(
            durable()?,
            before,
            "record, outbox and sequence roll back together"
        );
        assert!(
            store
                .get(TaskLookup {
                    id: "z-commit".into()
                })?
                .is_none()
        );
        assert_eq!(store.scan(TaskQuery::default())?.sequence, 1);
        assert!(matches!(
            commit("b")?,
            CommitResult::Committed { sequence: 2, .. }
        ));
        assert_eq!(durable()?.0, 2);
        Ok(())
    }

    #[test]
    fn records_at_the_admitted_nesting_limit_remain_readable() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let store = Store::open(&directory.path().join("tasks.sqlite"))?;
        store.claim_owner(StorageOwner {
            id: "unit-owner".into(),
        })?;
        let nested = |depth| {
            let mut value = serde_json::Value::Null;
            for _ in 0..depth {
                value = serde_json::json!([value]);
            }
            value
        };
        // A transaction nests input and checkpoint within four containers and a completed
        // value within five, so these are the deepest admitted values. Each stored record then
        // holds 125 containers, within the 127 that the trusted typed decode accepts.
        let mut deep = pending("deep");
        deep.record.input = nested(124);
        deep.record.checkpoint = nested(124);
        let mut completing = pending("completing");
        completing.record.status = TaskStatus::Completing;
        completing.record.outcome =
            Some(swarmie_task_contracts::TaskOutcome::Completed { value: nested(123) });
        let writes = vec![deep, completing];
        store.commit(TaskTransaction {
            owner: "unit-owner".into(),
            writes: writes.clone(),
        })?;
        let mut records: Vec<_> = writes.into_iter().map(|write| write.record).collect();
        records.sort_by(|left, right| left.id.cmp(&right.id));
        for record in &records {
            assert_eq!(
                store
                    .get(TaskLookup {
                        id: record.id.clone()
                    })?
                    .as_ref(),
                Some(record)
            );
        }
        assert_eq!(store.scan(TaskQuery::default())?.records, records);
        let mut deeper = pending("deeper");
        deeper.record.input = nested(125);
        assert!(matches!(
            store.commit(TaskTransaction {
                owner: "unit-owner".into(),
                writes: vec![deeper],
            }),
            Err(Error::Invalid(_))
        ));
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
