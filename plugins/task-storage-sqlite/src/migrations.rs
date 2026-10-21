use rusqlite::{Connection, OptionalExtension, TransactionBehavior};

use crate::store::{Error, Result};

const VERSION: i64 = 3;
const APPLICATION_ID: i64 = 0x5357544b;

/// Refuses unrelated or newer databases and installs the complete schema atomically.
pub(crate) fn apply(
    connection: &mut Connection,
    legacy_owners: &std::collections::BTreeMap<String, String>,
) -> Result<()> {
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
    if version < 2 {
        transaction.execute_batch(
            "CREATE TABLE task_outbox (
                sequence INTEGER NOT NULL,
                ordinal INTEGER NOT NULL,
                event TEXT NOT NULL,
                bytes INTEGER NOT NULL CHECK (bytes > 0),
                PRIMARY KEY(sequence, ordinal)
             ) WITHOUT ROWID;
             CREATE TABLE task_outbox_metadata (
                singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
                retained_events INTEGER NOT NULL,
                retained_bytes INTEGER NOT NULL,
                floor_sequence INTEGER NOT NULL,
                floor_ordinal INTEGER NOT NULL
             );
             INSERT INTO task_outbox_metadata SELECT 1, 0, 0, sequence, CASE WHEN sequence=0 THEN -1 ELSE 255 END FROM task_storage_metadata WHERE singleton=1;
             CREATE TABLE task_outbox_consumers (
                id TEXT PRIMARY KEY COLLATE BINARY,
                ack_sequence INTEGER NOT NULL,
                ack_ordinal INTEGER NOT NULL,
                delivered_sequence INTEGER NOT NULL,
                delivered_ordinal INTEGER NOT NULL
             ) WITHOUT ROWID;",
        )?;
        transaction.pragma_update(None, "user_version", VERSION)?;
    }
    if version < 3 {
        transaction
            .execute_batch("ALTER TABLE task_outbox_consumers ADD COLUMN owner_facet TEXT;")?;
        transaction.pragma_update(None, "user_version", VERSION)?;
    }
    // Validate the complete operator request before adopting any row. A failure rolls back
    // schema changes and prior data together; no legacy owner is inferred from first use.
    for (id, owner) in legacy_owners {
        swarmie_task_contracts::validate_identity(id)
            .map_err(|error| Error::Invalid(error.to_string()))?;
        swarmie_task_contracts::validate_task_outbox_owner_facet(owner)
            .map_err(|error| Error::Invalid(error.to_string()))?;
        let existing = transaction
            .query_row(
                "SELECT owner_facet FROM task_outbox_consumers WHERE id=?1",
                [id],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?;
        match existing {
            None => {
                return Err(Error::Invalid(
                    "legacy ownership adoption requires an existing consumer".into(),
                ));
            }
            Some(Some(existing)) if existing != *owner => return Err(Error::OutboxOwnerMismatch),
            _ => {}
        }
    }
    for (id, owner) in legacy_owners {
        transaction.execute(
            "UPDATE task_outbox_consumers SET owner_facet=?1 WHERE id=?2 AND owner_facet IS NULL",
            rusqlite::params![owner, id],
        )?;
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
