use rusqlite::{Connection, OptionalExtension, TransactionBehavior};

use crate::store::{Error, Result};

const VERSION: i64 = 1;
const APPLICATION_ID: i64 = 0x5357544b;

/// Refuses unrelated or newer databases and installs the complete schema atomically.
pub(crate) fn apply(connection: &mut Connection) -> Result<()> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let application: i64 =
        transaction.pragma_query_value(None, "application_id", |row| row.get(0))?;
    let version: i64 = transaction.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if application != 0 && application != APPLICATION_ID {
        return Err(Error::Unsupported(
            "the database belongs to another application".into(),
        ));
    }
    if version > VERSION
        || (application == APPLICATION_ID && version == 0)
        || (version != 0 && application != APPLICATION_ID)
    {
        return Err(Error::Unsupported(format!(
            "unsupported task storage schema version {version}"
        )));
    }
    if version == 0 {
        let tables: i64 = transaction.query_row(
            "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
            [],
            |row| row.get(0),
        )?;
        if tables != 0 {
            return Err(Error::Unsupported(
                "task storage requires a dedicated empty database".into(),
            ));
        }
        transaction.execute_batch(
            "CREATE TABLE task_storage_metadata (
                singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
                sequence INTEGER NOT NULL CHECK (sequence >= 0),
                store_id TEXT NOT NULL
             );
             CREATE TABLE task_records (
                id TEXT PRIMARY KEY COLLATE BINARY,
                revision INTEGER NOT NULL CHECK (revision >= 1),
                status TEXT NOT NULL,
                parent TEXT,
                record TEXT NOT NULL
             ) WITHOUT ROWID;
             CREATE INDEX task_records_status ON task_records(status, id);
             CREATE INDEX task_records_parent ON task_records(parent, id);",
        )?;
        transaction.execute(
            "INSERT INTO task_storage_metadata(singleton, sequence, store_id) VALUES(1, 0, ?1)",
            [uuid::Uuid::new_v4().to_string()],
        )?;
        transaction.pragma_update(None, "application_id", APPLICATION_ID)?;
        transaction.pragma_update(None, "user_version", VERSION)?;
    }
    let metadata: Option<(i64, String)> = transaction
        .query_row(
            "SELECT sequence, store_id FROM task_storage_metadata WHERE singleton = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    if metadata.is_none_or(|(sequence, identity)| sequence < 0 || identity.is_empty()) {
        return Err(Error::Corrupt(
            "task storage metadata is missing or invalid".into(),
        ));
    }
    transaction.commit()?;
    Ok(())
}
