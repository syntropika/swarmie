//! Read-only ownership admission precedes journal changes; initialization is atomic.
use crate::database::{io, metadata};
use rusqlite::{Connection, TransactionBehavior};
use tembra::{Result, RuntimeError};

const APPLICATION: i64 = 0x53575345;
const VERSION: i64 = 2;

fn validate(connection: &Connection) -> Result<i64> {
    let application: i64 = connection
        .pragma_query_value(None, "application_id", |row| row.get(0))
        .map_err(io)?;
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(io)?;
    match (application, version) {
        (0, 0) => {
            let objects: i64 = connection
                .query_row(
                    "SELECT count(*) FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'",
                    [],
                    |row| row.get(0),
                )
                .map_err(io)?;
            if objects != 0 {
                return Err(unsupported("a dedicated empty database is required"));
            }
        }
        (APPLICATION, VERSION) => {
            // Prepare every required table/column before WAL can alter an incompatible file.
            validate_domain(connection)?;
            metadata(connection)?;
        }
        _ => return Err(unsupported("foreign or unsupported schema; data retained")),
    }
    Ok(version)
}

/// A deferred transaction reads a consistent schema without writing or changing journal mode.
pub(crate) fn admit(connection: &mut Connection) -> Result<()> {
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Deferred)
        .map_err(io)?;
    validate(&transaction)?;
    transaction.commit().map_err(io)
}

pub(crate) fn initialize(connection: &mut Connection) -> Result<()> {
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(io)?;
    let version = validate(&transaction)?;
    if version == 0 {
        initialize_domain(&transaction)?;
        transaction
            .pragma_update(None, "application_id", APPLICATION)
            .map_err(io)?;
        transaction
            .pragma_update(None, "user_version", VERSION)
            .map_err(io)?;
    }
    transaction.commit().map_err(io)
}

fn unsupported(message: impl Into<String>) -> RuntimeError {
    RuntimeError::unavailable("session_store_schema", message)
}

/// Creates only SessionStore tables in the actual owner schema transaction.
pub(crate) fn initialize_domain(connection: &Connection) -> Result<()> {
    connection.execute_batch(
            "CREATE TABLE session_store(singleton INTEGER PRIMARY KEY CHECK(singleton=1), store_id TEXT NOT NULL, sequence INTEGER NOT NULL CHECK(sequence BETWEEN 0 AND 9007199254740991));
             CREATE TABLE sessions(id TEXT PRIMARY KEY, head INTEGER NOT NULL CHECK(head BETWEEN 0 AND 9007199254740991), metadata TEXT NOT NULL CHECK(length(CAST(metadata AS BLOB))<=65536)) WITHOUT ROWID;
             CREATE TABLE session_events(session_id TEXT NOT NULL REFERENCES sessions(id), sequence INTEGER NOT NULL CHECK(sequence BETWEEN 1 AND 9007199254740991), event_id TEXT NOT NULL, conversation_id TEXT, turn_id TEXT, event TEXT NOT NULL CHECK(length(CAST(event AS BLOB))<=262144), PRIMARY KEY(session_id, sequence), UNIQUE(session_id,event_id)) WITHOUT ROWID;
             CREATE INDEX session_events_turn ON session_events(session_id,turn_id,sequence);
             CREATE INDEX session_events_conversation ON session_events(session_id,conversation_id,sequence);
             CREATE INDEX session_events_conversation_turn ON session_events(session_id,conversation_id,turn_id,sequence);
             CREATE TABLE session_batches(session_id TEXT NOT NULL REFERENCES sessions(id), batch_id TEXT NOT NULL, request TEXT NOT NULL, receipt TEXT NOT NULL, PRIMARY KEY(session_id,batch_id)) WITHOUT ROWID;
             CREATE TRIGGER immutable_events_update BEFORE UPDATE ON session_events BEGIN SELECT RAISE(ABORT,'immutable event'); END;
             CREATE TRIGGER immutable_events_delete BEFORE DELETE ON session_events BEGIN SELECT RAISE(ABORT,'immutable event'); END;
             CREATE TRIGGER immutable_batches_update BEFORE UPDATE ON session_batches BEGIN SELECT RAISE(ABORT,'immutable receipt'); END;
             CREATE TRIGGER immutable_batches_delete BEFORE DELETE ON session_batches BEGIN SELECT RAISE(ABORT,'immutable receipt'); END;
             CREATE TRIGGER immutable_metadata BEFORE UPDATE OF id,metadata ON sessions BEGIN SELECT RAISE(ABORT,'immutable session'); END;
             CREATE TRIGGER immutable_sessions_delete BEFORE DELETE ON sessions BEGIN SELECT RAISE(ABORT,'retained session'); END;"
        ).map_err(io)?;

    connection
        .execute(
            "INSERT INTO session_store VALUES(1,?1,0)",
            [uuid::Uuid::new_v4().to_string()],
        )
        .map_err(io)?;
    Ok(())
}

/// Validates the current logical domain without file-level header checks.
pub(crate) fn validate_domain(connection: &Connection) -> Result<()> {
    for sql in [
        "SELECT id,head,metadata FROM sessions LIMIT 0",
        "SELECT session_id,sequence,event_id,conversation_id,turn_id,event FROM session_events LIMIT 0",
        "SELECT session_id,batch_id,request,receipt FROM session_batches LIMIT 0",
    ] {
        connection
            .prepare(sql)
            .map_err(|error| unsupported(error.to_string()))?;
    }
    metadata(connection)?;
    Ok(())
}
