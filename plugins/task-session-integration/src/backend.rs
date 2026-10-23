//! Synchronous task-history projection inside the physical owner's existing transaction.

use rusqlite::{Connection, OptionalExtension};
use swarmie_session_contracts::AppendReceipt;
use swarmie_task_contracts::{TaskRecord, TaskSessionAppend, validate_identity};
use swarmie_task_session_contracts::{BindingAdmission, BindingDisposition, BindingKey};
use tembra::Result;

use crate::{
    admission::{check_lineage, check_task, invalid},
    mapping, store,
};

fn active_pair_in(
    connection: &Connection,
    task_store_id: &str,
    session_store_id: &str,
) -> Result<bool> {
    if connection.is_autocommit() {
        return Err(invalid(
            "binding_transaction",
            "local history admission requires the caller's active transaction",
        ));
    }
    let configured: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name='binding_configuration')",
        [], |row| row.get(0),
    ).map_err(store::io)?;
    if !configured {
        return Ok(false);
    }
    let pins: Option<(String, String)> = connection
        .query_row(
            "SELECT task_store_id,session_store_id FROM binding_configuration WHERE singleton=1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(store::io)?;
    let Some(pins) = pins else {
        return Ok(false);
    };
    validate_identity(task_store_id)?;
    swarmie_session_contracts::identity(session_store_id)?;
    if pins != (task_store_id.to_owned(), session_store_id.to_owned()) {
        return Err(invalid(
            "binding_store_mismatch",
            "local history binding pins differ from the selected store pair",
        ));
    }
    Ok(true)
}

/// Checks explicit history against the durable route before the joint writer appends it.
/// A generic unconfigured catalog admits no route policy. With configured pins, every event
/// must name a routed task and its exact immutable session, conversation and turn.
/// This function reads only; it neither activates intent nor changes consumer positions.
///
/// # Errors
/// Refuses use outside an active transaction, mismatched store pairs, absent or excluded
/// routes, unsupported routing versions and any missing or mismatched correlation identity.
pub fn validate_history_in(
    connection: &Connection,
    task_store_id: &str,
    session_store_id: &str,
    history: &[TaskSessionAppend],
) -> Result<()> {
    if !active_pair_in(connection, task_store_id, session_store_id)? {
        return Ok(());
    }
    for history in history {
        if history.session_store_id != session_store_id {
            return Err(invalid(
                "binding_store_mismatch",
                "explicit history targets another SessionStore",
            ));
        }
        for event in &history.append.events {
            let task_id = event.correlation.task_id.as_deref().ok_or_else(|| {
                invalid(
                    "binding_fact_mismatch",
                    "explicit history must name its routed task",
                )
            })?;
            validate_identity(task_id)?;
            let key = BindingKey {
                task_store_id: task_store_id.to_owned(),
                task_id: task_id.to_owned(),
            };
            let binding = match store::read(connection, &key)? {
                Some(BindingDisposition::Routed { binding }) => binding,
                Some(BindingDisposition::Excluded { .. }) => {
                    return Err(invalid(
                        "binding_excluded",
                        "excluded work cannot append routed task history",
                    ));
                }
                None => {
                    return Err(invalid(
                        "binding_missing",
                        "explicit history requires a durable task route",
                    ));
                }
            };
            if binding.routing_version != 1 {
                return Err(invalid(
                    "binding_version",
                    "unsupported immutable routing version",
                ));
            }
            let destination = &binding.destination;
            if destination.session_store_id != session_store_id
                || destination.session_id != history.append.session_id
                || event.correlation.conversation_id.as_deref()
                    != Some(destination.conversation_id.as_str())
                || event.correlation.turn_id.as_deref() != Some(destination.turn_id.as_str())
            {
                return Err(invalid(
                    "binding_destination_mismatch",
                    "explicit history differs from its immutable session, conversation or turn route",
                ));
            }
        }
    }
    Ok(())
}

/// Projects this transaction's exact structural task facts using the ordinary frozen mapping.
/// An absent binding schema or an uninitialized binding catalog leaves generic joint stores
/// inactive. Once configured, every written task requires a valid route or explicit exclusion.
/// The caller must roll back its entire transaction on error and reconcile State after COMMIT.
///
/// This function never begins or commits a transaction, invokes References, or advances any
/// consumer position. The Facet-owned publisher later deduplicates these batches and owns ACKs.
///
/// # Errors
/// Refuses autocommit use, another configured store pair, missing or inconsistent facts/routes,
/// invalid lineage and SessionStore append refusals. Earlier SQL changes in the caller's
/// transaction remain uncommitted; only the caller can roll them back.
pub fn project_task_commit_in(
    connection: &Connection,
    task_store_id: &str,
    session_store_id: &str,
    sequence: u64,
    records: &[TaskRecord],
) -> Result<Vec<AppendReceipt>> {
    if !active_pair_in(connection, task_store_id, session_store_id)? {
        return Ok(vec![]);
    }
    let facts = swarmie_task_storage_sqlite::backend::committed_facts_in(
        connection,
        task_store_id,
        sequence,
    )?;
    if facts.len() != records.len()
        || facts.iter().zip(records).any(|(fact, record)| {
            fact.task_id != record.id
                || fact.revision != record.revision
                || fact.current_status != record.status
        })
    {
        return Err(invalid(
            "binding_fact_mismatch",
            "structural facts differ from this transaction's written records",
        ));
    }
    let mut receipts = Vec::with_capacity(facts.len());
    for (fact, record) in facts.iter().zip(records) {
        let key = BindingKey {
            task_store_id: task_store_id.to_owned(),
            task_id: fact.task_id.clone(),
        };
        match store::read(connection, &key)? {
            None => {
                return Err(invalid(
                    "binding_missing",
                    "local projection requires an explicit disposition for every committed task",
                ));
            }
            Some(BindingDisposition::Excluded { .. }) => {}
            Some(BindingDisposition::Routed { binding }) => {
                if binding.destination.session_store_id != session_store_id {
                    return Err(invalid(
                        "binding_store_mismatch",
                        "the immutable route targets another SessionStore",
                    ));
                }
                check_task(&binding, record)?;
                let parent = binding
                    .parent_task_id
                    .as_ref()
                    .map(|id| {
                        let key = BindingKey {
                            task_store_id: task_store_id.to_owned(),
                            task_id: id.clone(),
                        };
                        match store::read(connection, &key)? {
                            Some(BindingDisposition::Routed { binding }) => Ok(*binding),
                            Some(BindingDisposition::Excluded { .. }) => Err(invalid(
                                "binding_excluded",
                                "excluded work cannot obtain effect admission",
                            )),
                            None => Err(invalid(
                                "binding_missing",
                                "no durable parent routing intent exists",
                            )),
                        }
                    })
                    .transpose()?;
                check_lineage(&binding, parent.as_ref())?;
                let append = mapping::append(&binding, fact)?;
                let receipt = swarmie_session_store_sqlite::backend::append_in(
                    connection,
                    session_store_id,
                    append.clone(),
                )?;
                mapping::receipt(&binding, &append, &receipt)?;
                if binding.admission == BindingAdmission::Pending {
                    store::activate_in(connection, *binding)?;
                }
                receipts.push(receipt);
            }
        }
    }
    Ok(receipts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;
    use serde_json::json;
    use swarmie_task_contracts::{TaskOutboxEvent, TaskOutboxEventId, TaskSpec};
    use swarmie_task_session_contracts::{SessionDestination, TaskSessionBinding};

    struct Fixture {
        connection: Connection,
        task_store_id: String,
        session_store_id: String,
        binding: TaskSessionBinding,
        record: TaskRecord,
    }

    impl Fixture {
        fn new() -> Result<Self> {
            let connection = Connection::open_in_memory().map_err(store::io)?;
            (swarmie_task_storage_sqlite::sqlite_schema().initialize)(&connection)?;
            (swarmie_session_store_sqlite::sqlite_schema().initialize)(&connection)?;
            store::initialize_schema(&connection)?;
            let task_store_id: String = connection
                .query_row("SELECT store_id FROM task_storage_metadata", [], |row| {
                    row.get(0)
                })
                .map_err(store::io)?;
            let session_store_id: String = connection
                .query_row("SELECT store_id FROM session_store", [], |row| row.get(0))
                .map_err(store::io)?;
            connection
                .execute("INSERT INTO sessions VALUES('chat',0,'{}')", [])
                .map_err(store::io)?;
            connection
                .execute(
                    "INSERT INTO binding_configuration VALUES(1,?1,?2,'publisher')",
                    params![task_store_id, session_store_id],
                )
                .map_err(store::io)?;
            let task = TaskSpec {
                id: "task".into(),
                kind: "fixture.projection".into(),
                definition_version: 1,
                input: json!({}),
                checkpoint: json!({}),
                phase: "start".into(),
            };
            let record = TaskRecord::new(task.clone(), None);
            let binding = TaskSessionBinding {
                key: BindingKey {
                    task_store_id: task_store_id.clone(),
                    task_id: task.id.clone(),
                },
                destination: SessionDestination {
                    session_store_id: session_store_id.clone(),
                    session_id: "chat".into(),
                    conversation_id: "conversation".into(),
                    turn_id: "turn".into(),
                },
                root_task_id: task.id.clone(),
                parent_task_id: None,
                routing_version: 1,
                task,
                admission: BindingAdmission::Pending,
            };
            connection
                .execute(
                    "INSERT INTO task_session_bindings VALUES(?1,?2,?3)",
                    params![
                        task_store_id,
                        record.id,
                        store::canonical(&BindingDisposition::Routed {
                            binding: Box::new(binding.clone())
                        })?
                    ],
                )
                .map_err(store::io)?;
            // This consumer belongs to the ordinary publisher. Native projection has no
            // authority to mark its facts delivered or acknowledged.
            connection.execute("INSERT INTO task_outbox_consumers VALUES('publisher',0,-1,0,-1,'fixture.publisher')", []).map_err(store::io)?;
            Ok(Self {
                connection,
                task_store_id,
                session_store_id,
                binding,
                record,
            })
        }
    }

    fn seed(
        connection: &Connection,
        task_store_id: &str,
        record: &TaskRecord,
        ordinal: u32,
        count: u32,
    ) -> Result<TaskOutboxEvent> {
        let fact = TaskOutboxEvent::from_records(
            TaskOutboxEventId {
                store_id: task_store_id.into(),
                sequence: 1,
                ordinal,
            },
            None,
            record,
            count,
            None,
        );
        let event = store::canonical(&fact)?;
        connection
            .execute(
                "INSERT INTO task_records VALUES(?1,?2,'pending',NULL,?3)",
                params![record.id, record.revision, store::canonical(record)?],
            )
            .map_err(store::io)?;
        connection
            .execute(
                "INSERT INTO task_outbox VALUES(1,?1,?2,?3)",
                params![ordinal, event, event.len()],
            )
            .map_err(store::io)?;
        connection
            .execute("UPDATE task_storage_metadata SET sequence=1", [])
            .map_err(store::io)?;
        Ok(fact)
    }

    #[test]
    fn projection_uses_exact_mapping_activates_intent_and_leaves_ack_to_its_owner() -> Result<()> {
        let mut fixture = Fixture::new()?;
        let transaction = fixture.connection.transaction().map_err(store::io)?;
        let fact = seed(&transaction, &fixture.task_store_id, &fixture.record, 0, 1)?;
        let expected = mapping::append(&fixture.binding, &fact)?;
        let receipts = project_task_commit_in(
            &transaction,
            &fixture.task_store_id,
            &fixture.session_store_id,
            1,
            std::slice::from_ref(&fixture.record),
        )?;
        assert_eq!(receipts.len(), 1);
        assert_eq!(receipts[0].batch_id, expected.batch_id);
        assert_eq!(
            project_task_commit_in(
                &transaction,
                &fixture.task_store_id,
                &fixture.session_store_id,
                1,
                std::slice::from_ref(&fixture.record)
            )?,
            receipts
        );
        let persisted: String = transaction
            .query_row("SELECT event FROM session_events", [], |row| row.get(0))
            .map_err(store::io)?;
        let event: swarmie_session_contracts::SessionEvent =
            serde_json::from_str(&persisted).map_err(store::io)?;
        assert_eq!(event.event, expected.events[0]);
        assert!(
            matches!(store::read(&transaction, &fixture.binding.key)?, Some(BindingDisposition::Routed { binding }) if binding.admission == BindingAdmission::Active)
        );
        let cursor: (u64, i64, u64, i64) = transaction.query_row("SELECT ack_sequence,ack_ordinal,delivered_sequence,delivered_ordinal FROM task_outbox_consumers WHERE id='publisher'", [], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?))).map_err(store::io)?;
        assert_eq!(cursor, (0, -1, 0, -1));
        transaction.commit().map_err(store::io)?;
        Ok(())
    }

    #[test]
    fn missing_active_route_rolls_back_tasks_facts_and_an_earlier_projection() -> Result<()> {
        let mut fixture = Fixture::new()?;
        let mut missing = fixture.record.clone();
        missing.id = "missing".into();
        let records = vec![fixture.record.clone(), missing];
        {
            let transaction = fixture.connection.transaction().map_err(store::io)?;
            for (ordinal, record) in records.iter().enumerate() {
                seed(
                    &transaction,
                    &fixture.task_store_id,
                    record,
                    ordinal as u32,
                    2,
                )?;
            }
            let error = project_task_commit_in(
                &transaction,
                &fixture.task_store_id,
                &fixture.session_store_id,
                1,
                &records,
            )
            .unwrap_err();
            assert_eq!(error.reason, "binding_missing");
            // The caller owns rollback; dropping the transaction reverses all domains.
        }
        let counts: (u64,u64,u64) = fixture.connection.query_row("SELECT (SELECT count(*) FROM task_records),(SELECT count(*) FROM task_outbox),(SELECT count(*) FROM session_events)", [], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?))).map_err(store::io)?;
        assert_eq!(counts, (0, 0, 0));
        assert!(
            matches!(store::read(&fixture.connection, &fixture.binding.key)?, Some(BindingDisposition::Routed { binding }) if binding.admission == BindingAdmission::Pending)
        );
        Ok(())
    }

    #[test]
    fn inactive_projection_still_requires_an_explicit_transaction() -> Result<()> {
        let mut connection = Connection::open_in_memory().map_err(store::io)?;
        assert_eq!(
            project_task_commit_in(&connection, "tasks", "sessions", 1, &[])
                .unwrap_err()
                .reason,
            "binding_transaction"
        );
        let transaction = connection.transaction().map_err(store::io)?;
        assert!(project_task_commit_in(&transaction, "tasks", "sessions", 1, &[])?.is_empty());
        store::initialize_schema(&transaction)?;
        assert!(project_task_commit_in(&transaction, "tasks", "sessions", 1, &[])?.is_empty());
        transaction.rollback().map_err(store::io)?;
        Ok(())
    }
}
