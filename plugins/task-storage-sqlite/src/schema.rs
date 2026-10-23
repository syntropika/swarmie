use rusqlite::{Connection, OptionalExtension, TransactionBehavior};

use crate::store::{Error, Result};

const VERSION: i64 = 3;
const APPLICATION_ID: i64 = 0x5357544b;

/// Validates the current file format before journal configuration or schema writes.
/// Unsupported files are retained unchanged.
pub(crate) fn admit(connection: &Connection) -> Result<()> {
    current_or_empty(connection).map(|_| ())
}

/// Creates the complete current schema in an empty file or opens the current schema unchanged.
/// Returns the committed sequence and immutable store identity. Other persisted formats are refused.
pub(crate) fn initialize(connection: &mut Connection) -> Result<(u64, String)> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if current_or_empty(&transaction)? {
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
             CREATE INDEX task_records_parent ON task_records(parent, id);
             CREATE TABLE task_outbox (
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
             INSERT INTO task_outbox_metadata VALUES(1, 0, 0, 0, -1);
             CREATE TABLE task_outbox_consumers (
                id TEXT PRIMARY KEY COLLATE BINARY,
                ack_sequence INTEGER NOT NULL,
                ack_ordinal INTEGER NOT NULL,
                delivered_sequence INTEGER NOT NULL,
                delivered_ordinal INTEGER NOT NULL,
                owner_facet TEXT NOT NULL
             ) WITHOUT ROWID;",
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
    let Some((sequence, store_id)) = metadata.and_then(|(sequence, store_id)| {
        u64::try_from(sequence)
            .ok()
            .filter(|_| !store_id.is_empty())
            .map(|sequence| (sequence, store_id))
    }) else {
        return Err(Error::Corrupt(
            "task storage metadata is missing or invalid".into(),
        ));
    };
    transaction.commit()?;
    Ok((sequence, store_id))
}

fn current_or_empty(connection: &Connection) -> Result<bool> {
    let application: i64 =
        connection.pragma_query_value(None, "application_id", |row| row.get(0))?;
    let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if application == 0 && version == 0 {
        let objects: i64 = connection.query_row(
            "SELECT count(*) FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%'",
            [],
            |row| row.get(0),
        )?;
        if objects == 0 {
            return Ok(true);
        }
    } else if application == APPLICATION_ID && version == VERSION {
        for sql in [
            "SELECT singleton,sequence,store_id FROM task_storage_metadata LIMIT 0",
            "SELECT id,revision,status,parent,record FROM task_records INDEXED BY task_records_status LIMIT 0",
            "SELECT id,revision,status,parent,record FROM task_records INDEXED BY task_records_parent LIMIT 0",
            "SELECT sequence,ordinal,event,bytes FROM task_outbox LIMIT 0",
            "SELECT singleton,retained_events,retained_bytes,floor_sequence,floor_ordinal FROM task_outbox_metadata LIMIT 0",
            "SELECT id,ack_sequence,ack_ordinal,delivered_sequence,delivered_ordinal,owner_facet FROM task_outbox_consumers LIMIT 0",
        ] {
            connection
                .prepare(sql)
                .map_err(|error| Error::Unsupported(error.to_string()))?;
        }
        let owner_required: bool = connection.query_row(
            "SELECT \"notnull\" FROM pragma_table_info('task_outbox_consumers') WHERE name='owner_facet'",
            [], |row| row.get(0),
        )?;
        if owner_required {
            return Ok(false);
        }
        return Err(Error::Unsupported(
            "current task storage requires a non-null consumer owner".into(),
        ));
    }
    Err(Error::Unsupported(format!(
        "unsupported task storage application {application} and schema version {version}"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;

    #[test]
    fn unsupported_versions_are_refused_before_journal_or_data_changes() -> Result<()> {
        for version in [1, 2, 4] {
            let directory = tempfile::tempdir()?;
            let path = directory.path().join("tasks.sqlite");
            let store = Store::open(&path)?;
            store.close()?;
            let connection = Connection::open(&path)?;
            connection.execute_batch("PRAGMA journal_mode=DELETE; INSERT INTO task_outbox_consumers VALUES('consumer',0,-1,0,-1,'fixture.owner');")?;
            connection.pragma_update(None, "user_version", version)?;
            connection.close().map_err(|(_, error)| error)?;
            let before = std::fs::read(&path)?;
            assert!(matches!(Store::open(&path), Err(Error::Unsupported(_))));
            assert_eq!(
                std::fs::read(&path)?,
                before,
                "refused schema must retain the entire file unchanged"
            );
        }
        Ok(())
    }

    #[test]
    fn current_schema_requires_consumer_owners_and_rejects_nullable_shape_unchanged() -> Result<()>
    {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("tasks.sqlite");
        let store = Store::open(&path)?;
        store.close()?;
        let connection = Connection::open(&path)?;
        assert!(
            connection
                .execute(
                    "INSERT INTO task_outbox_consumers VALUES('consumer',0,-1,0,-1,NULL)",
                    []
                )
                .is_err()
        );
        connection.execute_batch("PRAGMA journal_mode=DELETE; ALTER TABLE task_outbox_consumers RENAME TO required_owners; CREATE TABLE task_outbox_consumers (id TEXT PRIMARY KEY,ack_sequence INTEGER NOT NULL,ack_ordinal INTEGER NOT NULL,delivered_sequence INTEGER NOT NULL,delivered_ordinal INTEGER NOT NULL,owner_facet TEXT) WITHOUT ROWID; INSERT INTO task_outbox_consumers VALUES('consumer',0,-1,0,-1,NULL); DROP TABLE required_owners;")?;
        connection.close().map_err(|(_, error)| error)?;
        let before = std::fs::read(&path)?;
        assert!(matches!(Store::open(&path), Err(Error::Unsupported(_))));
        assert_eq!(std::fs::read(&path)?, before);
        Ok(())
    }
}
