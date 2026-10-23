use std::collections::BTreeMap;

use rusqlite::{Connection, OptionalExtension, params};
use swarmie_task_contracts::{
    MAX_PORTABLE_INTEGER, MAX_TASK_OUTBOX_EVENT_BYTES, MAX_TASK_PAGE_BYTES,
    TaskOutboxAcknowledgement, TaskOutboxCompaction, TaskOutboxConsumer, TaskOutboxConsumerState,
    TaskOutboxEvent, TaskOutboxEventId, TaskOutboxPage, TaskOutboxQuery, TaskOutboxRetention,
    TaskOutboxStatus, TaskRecord, TaskTransaction, task_wire_bytes, validate_identity,
};

use crate::{
    TaskOutboxLimits,
    store::{Error, Result, Store, Transaction},
};

type Position = (u64, i64);
const START: Position = (0, -1);

fn invalid(message: &str) -> Error {
    Error::Invalid(message.into())
}

fn identity(value: &str) -> Result<()> {
    validate_identity(value).map_err(|error| Error::Invalid(error.to_string()))
}

fn position(id: &TaskOutboxEventId, store_id: &str) -> Result<Position> {
    if id.store_id != store_id
        || id.sequence == 0
        || id.sequence > MAX_PORTABLE_INTEGER
        || id.ordinal >= 256
    {
        return Err(invalid(
            "outbox cursor belongs to another store or is invalid",
        ));
    }
    Ok((id.sequence, i64::from(id.ordinal)))
}

fn event_id(position: Position, store_id: &str) -> Option<TaskOutboxEventId> {
    (position != START).then(|| TaskOutboxEventId {
        store_id: store_id.into(),
        sequence: position.0,
        ordinal: position.1 as u32,
    })
}

fn usage(connection: &Connection) -> Result<(u64, u64, Position)> {
    Ok(connection.prepare_cached("SELECT retained_events, retained_bytes, floor_sequence, floor_ordinal FROM task_outbox_metadata WHERE singleton=1")?.query_row([], |row| Ok((row.get(0)?, row.get(1)?, (row.get(2)?, row.get(3)?))))?)
}

fn consumer(connection: &Connection, id: &str, owner: &str) -> Result<(Position, Position)> {
    let (actual,ack,delivered) = connection.prepare_cached(
        "SELECT owner_facet,ack_sequence,ack_ordinal,delivered_sequence,delivered_ordinal FROM task_outbox_consumers WHERE id=?1")?.query_row([id],
        |row| Ok((row.get::<_,String>(0)?,(row.get(1)?,row.get(2)?),(row.get(3)?,row.get(4)?))))
        .optional()?.ok_or_else(|| invalid("outbox consumer is not registered"))?;
    swarmie_task_contracts::validate_task_outbox_owner_facet(&actual)
        .map_err(|error| Error::Corrupt(error.to_string()))?;
    if actual != owner {
        Err(Error::OutboxOwnerMismatch)
    } else {
        Ok((ack, delivered))
    }
}

fn owner_name(owner: &str) -> Result<()> {
    swarmie_task_contracts::validate_task_outbox_owner_facet(owner)
        .map_err(|error| Error::Invalid(error.to_string()))
}

fn boundary(connection: &Connection, position: Position, floor: Position) -> Result<()> {
    if position == START || position == floor {
        return Ok(());
    }
    let exists: bool = connection
        .prepare_cached(
            "SELECT EXISTS(SELECT 1 FROM task_outbox WHERE sequence=?1 AND ordinal=?2)",
        )?
        .query_row(params![position.0, position.1], |row| row.get(0))?;
    if !exists {
        return Err(invalid("outbox position is not a retained event boundary"));
    }
    Ok(())
}

pub(crate) fn prepare_events(
    connection: &Connection,
    batch: &TaskTransaction,
    previous: &BTreeMap<String, Option<TaskRecord>>,
    store_id: &str,
    sequence: u64,
    limits: TaskOutboxLimits,
) -> Result<Vec<(u64, u32, String)>> {
    let mut events = Vec::with_capacity(batch.writes.len());
    let mut bytes = 0_u64;
    for (ordinal, write) in batch.writes.iter().enumerate() {
        let id = TaskOutboxEventId {
            store_id: store_id.into(),
            sequence,
            ordinal: ordinal as u32,
        };
        let event = TaskOutboxEvent::from_records(
            id,
            previous.get(&write.record.id).and_then(Option::as_ref),
            &write.record,
            batch.writes.len() as u32,
            write.provenance,
        );
        let portable = tembra::contracts::to_portable_value(&event)
            .map_err(|error| Error::Invalid(error.to_string()))?;
        let encoded = tembra::contracts::canonical_json(&portable)
            .map_err(|error| Error::Invalid(error.to_string()))?;
        if encoded.len() > MAX_TASK_OUTBOX_EVENT_BYTES {
            return Err(Error::TooLarge("outbox event exceeds 4096 bytes".into()));
        }
        bytes += encoded.len() as u64;
        events.push((sequence, ordinal as u32, encoded));
    }
    let (retained_events, retained_bytes, _) = usage(connection)?;
    if retained_events.saturating_add(events.len() as u64) > limits.max_events
        || retained_bytes.saturating_add(bytes) > limits.max_bytes
    {
        return Err(Error::OutboxFull);
    }
    Ok(events)
}

pub(crate) fn append_events(connection: &Connection, events: &[(u64, u32, String)]) -> Result<()> {
    let mut statement = connection.prepare_cached(
        "INSERT INTO task_outbox(sequence, ordinal, event, bytes) VALUES(?1, ?2, ?3, ?4)",
    )?;
    let mut bytes = 0_u64;
    for (sequence, ordinal, encoded) in events {
        bytes += encoded.len() as u64;
        statement.execute(params![sequence, ordinal, encoded, encoded.len() as u64])?;
    }
    connection.prepare_cached("UPDATE task_outbox_metadata SET retained_events=retained_events+?1, retained_bytes=retained_bytes+?2 WHERE singleton=1")?.execute(params![events.len() as u64, bytes])?;
    Ok(())
}

impl Store {
    pub(crate) fn outbox_status(&self) -> Result<TaskOutboxStatus> {
        let connection = self.connection.borrow();
        let transaction = Transaction::deferred(connection.as_ref().ok_or(Error::Closed)?)?;
        let store_id = &self.store_id;
        let (retained_events, retained_bytes, floor) = usage(&transaction)?;
        let positions = {
            let mut statement = transaction.prepare_cached("SELECT id,ack_sequence,ack_ordinal,delivered_sequence,delivered_ordinal,owner_facet FROM task_outbox_consumers ORDER BY id COLLATE BINARY LIMIT 65")?;
            statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        (row.get::<_, u64>(1)?, row.get::<_, i64>(2)?),
                        (row.get::<_, u64>(3)?, row.get::<_, i64>(4)?),
                        row.get::<_, String>(5)?,
                    ))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?
        };
        for (_, _, _, owner) in &positions {
            swarmie_task_contracts::validate_task_outbox_owner_facet(owner)
                .map_err(|error| Error::Corrupt(error.to_string()))?;
        }
        if positions.len() > 64 {
            return Err(Error::Corrupt(
                "outbox exceeds bounded consumer count".into(),
            ));
        }
        let acknowledged = positions.iter().map(|(_, ack, _, _)| *ack).min();
        let compactable_events = if let Some(ack) = acknowledged {
            transaction
                .prepare_cached(
                    "SELECT count(*) FROM task_outbox WHERE (sequence,ordinal)<=(?1,?2)",
                )?
                .query_row(params![ack.0, ack.1], |row| row.get::<_, u64>(0))?
        } else {
            0
        };
        let pending_events = retained_events
            .checked_sub(compactable_events)
            .ok_or_else(|| {
                Error::Corrupt("outbox retained usage disagrees with event rows".into())
            })?;
        let consumers = positions
            .into_iter()
            .map(
                |(consumer_id, ack, delivered, owner_facet)| TaskOutboxConsumerState {
                    owner_facet,
                    consumer_id,
                    store_id: store_id.clone(),
                    acknowledged: event_id(ack, store_id),
                    delivered: event_id(delivered, store_id),
                    retained_after: event_id(floor, store_id),
                },
            )
            .collect();
        let status = TaskOutboxStatus {
            store_id: store_id.clone(),
            limits: self.outbox_limits,
            retained_after: event_id(floor, store_id),
            retained_events,
            retained_bytes,
            compactable_events,
            pending_events,
            consumers,
        };
        swarmie_task_contracts::validate_task_payload_size(&status)
            .map_err(|error| Error::TooLarge(error.to_string()))?;
        transaction.commit()?;
        Ok(status)
    }

    pub(crate) fn register_outbox_consumer(
        &self,
        request: TaskOutboxConsumer,
        owner: &str,
    ) -> Result<TaskOutboxConsumerState> {
        identity(&request.id)?;
        owner_name(owner)?;
        let connection = self.connection.borrow();
        let transaction = Transaction::immediate(connection.as_ref().ok_or(Error::Closed)?)?;
        let store_id = &self.store_id;
        let (_, _, floor) = usage(&transaction)?;
        let exists: bool = transaction
            .prepare_cached("SELECT EXISTS(SELECT 1 FROM task_outbox_consumers WHERE id=?1)")?
            .query_row([&request.id], |row| row.get(0))?;
        if !exists {
            let count: u32 = transaction
                .prepare_cached("SELECT count(*) FROM task_outbox_consumers")?
                .query_row([], |row| row.get(0))?;
            if count >= self.outbox_limits.max_consumers {
                return Err(Error::OutboxFull);
            }
            transaction.prepare_cached(
                "INSERT INTO task_outbox_consumers(id,ack_sequence,ack_ordinal,delivered_sequence,delivered_ordinal,owner_facet) VALUES(?1, ?2, ?3, ?2, ?3, ?4)",
            )?.execute(params![request.id, floor.0, floor.1, owner])?;
        }
        let (ack, delivered) = consumer(&transaction, &request.id, owner)?;
        let state = TaskOutboxConsumerState {
            owner_facet: owner.into(),
            consumer_id: request.id,
            store_id: store_id.clone(),
            acknowledged: event_id(ack, store_id),
            delivered: event_id(delivered, store_id),
            retained_after: event_id(floor, store_id),
        };
        transaction.commit()?;
        Ok(state)
    }

    pub(crate) fn read_outbox(
        &self,
        request: TaskOutboxQuery,
        owner: &str,
    ) -> Result<TaskOutboxPage> {
        identity(&request.consumer_id)?;
        owner_name(owner)?;
        if !(1..=256).contains(&request.limit) {
            return Err(invalid("outbox page limit requires 1..=256"));
        }
        let connection = self.connection.borrow();
        let transaction = Transaction::immediate(connection.as_ref().ok_or(Error::Closed)?)?;
        let store_id = &self.store_id;
        let (_, _, floor) = usage(&transaction)?;
        let (ack, delivered) = consumer(&transaction, &request.consumer_id, owner)?;
        let after = request
            .after
            .as_ref()
            .map(|id| position(id, store_id))
            .transpose()?
            .unwrap_or(ack);
        if after < ack || after > delivered {
            return Err(invalid(
                "outbox continuation must lie in the consumer's delivered unacknowledged prefix",
            ));
        }
        boundary(&transaction, after, floor)?;
        let mut page = TaskOutboxPage {
            store_id: store_id.clone(),
            retained_after: event_id(floor, store_id),
            events: vec![],
            next: event_id(after, store_id),
            acknowledged: event_id(ack, store_id),
        };
        let mut last = after;
        {
            let mut statement = transaction.prepare_cached("SELECT sequence, ordinal, event FROM task_outbox WHERE (sequence,ordinal)>(?1,?2) ORDER BY sequence,ordinal LIMIT ?3")?;
            let mut rows = statement.query(params![after.0, after.1, request.limit])?;
            let envelope =
                task_wire_bytes(&page).map_err(|error| Error::Invalid(error.to_string()))?;
            let mut bytes = envelope;
            while let Some(row) = rows.next()? {
                let sequence: u64 = row.get(0)?;
                let ordinal: i64 = row.get(1)?;
                let encoded: String = row.get(2)?;
                if bytes + encoded.len() + 512 > MAX_TASK_PAGE_BYTES {
                    break;
                }
                let event = decode_event(&encoded)?;
                let expected = event_id((sequence, ordinal), store_id)
                    .ok_or_else(|| Error::Corrupt("invalid outbox event identity".into()))?;
                if event.id != expected {
                    return Err(Error::Corrupt(
                        "outbox event and indexed identity disagree".into(),
                    ));
                }
                last = (sequence, ordinal);
                bytes += encoded.len() + 1;
                page.events.push(event);
            }
        }
        page.next = event_id(last, store_id);
        if task_wire_bytes(&page).map_err(|error| Error::Invalid(error.to_string()))?
            > MAX_TASK_PAGE_BYTES
        {
            return Err(Error::TooLarge(
                "outbox page exceeds portable budget".into(),
            ));
        }
        if last > delivered {
            transaction.prepare_cached("UPDATE task_outbox_consumers SET delivered_sequence=?1, delivered_ordinal=?2 WHERE id=?3")?.execute(params![last.0, last.1, request.consumer_id])?;
        }
        transaction.commit()?;
        Ok(page)
    }

    pub(crate) fn acknowledge_outbox(
        &self,
        request: TaskOutboxAcknowledgement,
        owner: &str,
    ) -> Result<()> {
        identity(&request.consumer_id)?;
        owner_name(owner)?;
        let connection = self.connection.borrow();
        let transaction = Transaction::immediate(connection.as_ref().ok_or(Error::Closed)?)?;
        let through = position(&request.through, &self.store_id)?;
        let (ack, delivered) = consumer(&transaction, &request.consumer_id, owner)?;
        if through > delivered {
            return Err(invalid("outbox acknowledgement exceeds delivered prefix"));
        }
        if through > ack {
            let (_, _, floor) = usage(&transaction)?;
            boundary(&transaction, through, floor)?;
            transaction
                .prepare_cached(
                    "UPDATE task_outbox_consumers SET ack_sequence=?1, ack_ordinal=?2 WHERE id=?3",
                )?
                .execute(params![through.0, through.1, request.consumer_id])?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub(crate) fn remove_outbox_consumer(
        &self,
        request: TaskOutboxConsumer,
        owner: &str,
    ) -> Result<()> {
        identity(&request.id)?;
        owner_name(owner)?;
        let connection = self.connection.borrow();
        let transaction = Transaction::immediate(connection.as_ref().ok_or(Error::Closed)?)?;
        let exists: bool = transaction
            .prepare_cached("SELECT EXISTS(SELECT 1 FROM task_outbox_consumers WHERE id=?1)")?
            .query_row([&request.id], |row| row.get(0))?;
        if exists {
            consumer(&transaction, &request.id, owner)?;
            transaction
                .prepare_cached("DELETE FROM task_outbox_consumers WHERE id=?1")?
                .execute([request.id])?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub(crate) fn compact_outbox(
        &self,
        request: TaskOutboxCompaction,
    ) -> Result<TaskOutboxRetention> {
        if !(1..=4096).contains(&request.limit) {
            return Err(invalid("outbox compaction limit requires 1..=4096"));
        }
        let connection = self.connection.borrow();
        let transaction = Transaction::immediate(connection.as_ref().ok_or(Error::Closed)?)?;
        let protected: Option<Position> = transaction.prepare_cached("SELECT ack_sequence, ack_ordinal FROM task_outbox_consumers ORDER BY ack_sequence,ack_ordinal LIMIT 1")?.query_row([], |row| Ok((row.get(0)?, row.get(1)?))).optional()?;
        let mut removed = 0_u32;
        if let Some(protected) = protected {
            let candidates = {
                let mut statement = transaction.prepare_cached("SELECT sequence,ordinal,bytes FROM task_outbox WHERE (sequence,ordinal)<=(?1,?2) ORDER BY sequence,ordinal LIMIT ?3")?;
                statement
                    .query_map(params![protected.0, protected.1, request.limit], |row| {
                        Ok((
                            row.get::<_, u64>(0)?,
                            row.get::<_, i64>(1)?,
                            row.get::<_, u64>(2)?,
                        ))
                    })?
                    .collect::<std::result::Result<Vec<_>, _>>()?
            };
            if let Some((sequence, ordinal, _)) = candidates.last() {
                let bytes: u64 = candidates.iter().map(|row| row.2).sum();
                removed = candidates.len() as u32;
                transaction
                    .prepare_cached("DELETE FROM task_outbox WHERE (sequence,ordinal)<=(?1,?2)")?
                    .execute(params![sequence, ordinal])?;
                transaction.prepare_cached("UPDATE task_outbox_metadata SET retained_events=retained_events-?1,retained_bytes=retained_bytes-?2,floor_sequence=?3,floor_ordinal=?4 WHERE singleton=1")?.execute(params![removed,bytes,sequence,ordinal])?;
            }
        }
        let (retained_events, retained_bytes, _) = usage(&transaction)?;
        transaction.commit()?;
        Ok(TaskOutboxRetention {
            removed,
            retained_events,
            retained_bytes,
        })
    }
}

fn decode_event(encoded: &str) -> Result<TaskOutboxEvent> {
    if encoded.len() > MAX_TASK_OUTBOX_EVENT_BYTES {
        return Err(Error::Corrupt("outbox event exceeds bound".into()));
    }
    let value = tembra::contracts::parse_json(encoded)
        .map_err(|error| Error::Corrupt(error.to_string()))?;
    let event: TaskOutboxEvent =
        serde_json::from_value(value).map_err(|error| Error::Corrupt(error.to_string()))?;
    for id in [&event.task_id, &event.task_kind, &event.phase] {
        identity(id).map_err(|error| Error::Corrupt(error.to_string()))?;
    }
    if let Some(parent) = &event.parent {
        identity(parent).map_err(|error| Error::Corrupt(error.to_string()))?;
    }
    if !(1..=256).contains(&event.transaction_size)
        || event.id.ordinal >= event.transaction_size
        || event.version != 2
        || event.revision == 0
        || event.revision > MAX_PORTABLE_INTEGER
        || event.definition_version == 0
    {
        return Err(Error::Corrupt("outbox record metadata is invalid".into()));
    }
    Ok(event)
}

#[cfg(test)]
mod tests {
    use super::*;
    use swarmie_task_contracts::{
        CommitResult, StorageOwner, TaskLifecycleOrigin, TaskLookup, TaskSpec,
        TaskTransitionProvenance, TaskWrite,
    };

    const OWNER: &str = "outbox-test-owner";
    fn pending(id: &str) -> TaskWrite {
        TaskWrite {
            provenance: None,
            expected_revision: None,
            record: TaskRecord::new(
                TaskSpec {
                    id: id.into(),
                    kind: "test.outbox".into(),
                    definition_version: 1,
                    input: serde_json::json!(null),
                    checkpoint: serde_json::json!({"phase":"ready"}),
                    phase: "ready".into(),
                },
                None,
            ),
        }
    }
    fn commit(store: &Store, writes: Vec<TaskWrite>) -> Result<CommitResult> {
        store.commit(TaskTransaction {
            owner: OWNER.into(),
            writes,
        })
    }
    fn register(store: &Store, id: &str) -> Result<TaskOutboxConsumerState> {
        store.register_outbox_consumer(TaskOutboxConsumer { id: id.into() }, OWNER)
    }
    fn read(
        store: &Store,
        id: &str,
        after: Option<TaskOutboxEventId>,
        limit: u32,
    ) -> Result<TaskOutboxPage> {
        store.read_outbox(
            TaskOutboxQuery {
                consumer_id: id.into(),
                after,
                limit,
            },
            OWNER,
        )
    }
    fn ack(store: &Store, id: &str, through: TaskOutboxEventId) -> Result<()> {
        store.acknowledge_outbox(
            TaskOutboxAcknowledgement {
                consumer_id: id.into(),
                through,
            },
            OWNER,
        )
    }
    fn compact(store: &Store) -> Result<TaskOutboxRetention> {
        store.compact_outbox(TaskOutboxCompaction { limit: 4096 })
    }
    fn open(path: &std::path::Path) -> Result<Store> {
        let store = Store::open(path)?;
        store.claim_owner(StorageOwner { id: OWNER.into() })?;
        Ok(store)
    }

    #[test]
    fn atomic_batch_order_cas_conflict_and_structural_metadata() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let store = open(&directory.path().join("tasks.sqlite"))?;
        register(&store, "observer")?;
        let mut parent = pending("z-parent");
        let mut child = pending("a-child");
        child.record.parent = Some(parent.record.id.clone());
        parent.record.input = serde_json::json!("x".repeat(100_000));
        parent.provenance = Some(TaskTransitionProvenance {
            origin: TaskLifecycleOrigin::Creation,
            checkpoint_committed: false,
        });
        commit(&store, vec![parent.clone(), child.clone()])?;
        let page = read(&store, "observer", None, 256)?;
        assert_eq!(page.events.len(), 2);
        assert_eq!(page.events[0].task_id, "z-parent");
        assert_eq!(page.events[0].version, 2);
        assert_eq!(page.events[0].provenance, parent.provenance);
        assert_eq!(page.events[1].version, 2);
        assert_eq!(
            page.events[1].provenance, None,
            "unannotated direct storage writes remain unknown"
        );
        parent.provenance = Some(TaskTransitionProvenance {
            origin: TaskLifecycleOrigin::Retry,
            checkpoint_committed: true,
        });
        assert_eq!(page.events[1].parent.as_deref(), Some("z-parent"));
        assert_eq!(page.events[0].id.sequence, page.events[1].id.sequence);
        assert_eq!(page.events[0].transaction_size, 2);
        assert_eq!(page.events[0].id.ordinal, 0);
        assert_eq!(page.events[1].id.ordinal, 1);
        assert!(task_wire_bytes(&page).expect("portable page") < 4096);
        assert!(matches!(
            commit(&store, vec![parent, pending("never-created")])?,
            CommitResult::Conflict { .. }
        ));
        assert!(
            read(&store, "observer", page.next.clone(), 256)?
                .events
                .is_empty()
        );
        let mut update = child;
        update.expected_revision = Some(1);
        update.record.revision = 2;
        update.record.checkpoint = serde_json::json!({"phase":"second"});
        update.record.phase = "second".into();
        update.record.cancellation_requested = true;
        update.provenance = Some(TaskTransitionProvenance {
            origin: TaskLifecycleOrigin::Handler,
            checkpoint_committed: true,
        });
        let expected_provenance = update.provenance;
        commit(&store, vec![update])?;
        let event = read(&store, "observer", page.next, 256)?.events.remove(0);
        assert!(event.checkpoint_changed);
        assert_eq!(event.provenance, expected_provenance);
        assert!(event.cancellation_requested);
        assert_eq!(
            event.previous_status,
            Some(swarmie_task_contracts::TaskStatus::Pending)
        );
        assert_eq!(event.revision, 2);
        Ok(())
    }

    #[test]
    fn late_sqlite_failure_rolls_back_records_events_usage_and_sequence() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("tasks.sqlite");
        let store = open(&path)?;
        register(&store, "observer")?;
        let raw = Connection::open(&path)?;
        raw.execute_batch("CREATE TRIGGER refuse_event BEFORE INSERT ON task_outbox WHEN NEW.ordinal=1 BEGIN SELECT RAISE(FAIL, 'injected outbox failure'); END;")?;
        let writes: Vec<_> = ["a", "b"]
            .into_iter()
            .map(|id| {
                let mut write = pending(id);
                write.provenance = Some(TaskTransitionProvenance {
                    origin: TaskLifecycleOrigin::Creation,
                    checkpoint_committed: false,
                });
                write
            })
            .collect();
        assert!(commit(&store, writes.clone()).is_err());
        assert!(read(&store, "observer", None, 256)?.events.is_empty());
        assert!(store.get(TaskLookup { id: "a".into() })?.is_none());
        assert_eq!(compact(&store)?.retained_events, 0);
        assert_eq!(store.scan(Default::default())?.sequence, 0);
        raw.execute_batch("DROP TRIGGER refuse_event;")?;
        commit(&store, writes)?;
        let retried = read(&store, "observer", None, 256)?;
        assert_eq!(retried.events.len(), 2);
        assert!(retried.events.iter().all(|event| event.version == 2
            && event.id.sequence == 1
            && event.provenance.is_some()));
        Ok(())
    }

    #[test]
    fn append_stable_pagination_ack_validation_and_idempotent_redelivery() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let store = open(&directory.path().join("tasks.sqlite"))?;
        let initial = register(&store, "observer")?;
        commit(&store, vec![pending("a"), pending("b")])?;
        let first = read(&store, "observer", None, 1)?;
        let first_id = first.next.clone().expect("event");
        let mut future = first_id.clone();
        future.ordinal = 1;
        assert!(read(&store, "observer", Some(future.clone()), 256).is_err());
        assert!(ack(&store, "observer", future).is_err());
        let mut wrong = first_id.clone();
        wrong.store_id = "another-store".into();
        assert!(ack(&store, "observer", wrong).is_err());
        commit(&store, vec![pending("c")])?;
        let rest = read(&store, "observer", first.next.clone(), 256)?;
        assert_eq!(
            rest.events
                .iter()
                .map(|event| event.task_id.as_str())
                .collect::<Vec<_>>(),
            ["b", "c"]
        );
        let mut forged = first_id.clone();
        forged.ordinal = 255;
        assert!(
            ack(&store, "observer", forged.clone()).is_err(),
            "nonexistent ordinal inside delivered range is refused"
        );
        assert!(read(&store, "observer", Some(forged), 256).is_err());
        let duplicate = read(&store, "observer", None, 256)?;
        assert_eq!(duplicate.events[0].id, first_id);
        assert_eq!(register(&store, "observer")?.store_id, initial.store_id);
        ack(&store, "observer", rest.next.clone().expect("tail"))?;
        ack(&store, "observer", first_id.clone())?;
        assert!(read(&store, "observer", Some(first_id), 256).is_err());
        assert!(read(&store, "observer", None, 256)?.events.is_empty());
        Ok(())
    }

    #[test]
    fn disconnected_consumer_and_no_consumers_protect_pending_history() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let store = open(&directory.path().join("tasks.sqlite"))?;
        commit(&store, vec![pending("a"), pending("b")])?;
        assert_eq!(compact(&store)?.removed, 0);
        register(&store, "online")?;
        register(&store, "offline")?;
        let page = read(&store, "online", None, 256)?;
        ack(&store, "online", page.next.expect("tail"))?;
        assert_eq!(compact(&store)?.removed, 0);
        let page = read(&store, "offline", None, 256)?;
        assert_eq!(page.events.len(), 2);
        ack(&store, "offline", page.events[0].id.clone())?;
        assert_eq!(compact(&store)?.removed, 1);
        assert_eq!(compact(&store)?.retained_events, 1);
        store.remove_outbox_consumer(
            TaskOutboxConsumer {
                id: "offline".into(),
            },
            OWNER,
        )?;
        let cleanup = compact(&store)?;
        assert_eq!(cleanup.removed, 1);
        assert_eq!(cleanup.retained_bytes, 0);
        let newcomer = register(&store, "new")?;
        assert!(newcomer.retained_after.is_some());
        assert_eq!(newcomer.acknowledged, newcomer.retained_after);
        assert!(read(&store, "new", None, 256)?.events.is_empty());
        commit(&store, vec![pending("c")])?;
        assert_eq!(
            read(&store, "new", newcomer.retained_after, 256)?
                .events
                .len(),
            1
        );
        Ok(())
    }

    #[test]
    fn durable_ack_and_delivery_resume_after_connection_reopen() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("tasks.sqlite");
        let store = open(&path)?;
        register(&store, "observer")?;
        commit(&store, vec![pending("a"), pending("b")])?;
        let page = read(&store, "observer", None, 256)?;
        ack(&store, "observer", page.events[0].id.clone())?;
        store.close()?;
        let store = open(&path)?;
        let resumed = register(&store, "observer")?;
        assert_eq!(resumed.delivered, page.next);
        assert_eq!(resumed.acknowledged, Some(page.events[0].id.clone()));
        let replay = read(&store, "observer", None, 256)?;
        assert_eq!(replay.events, [page.events[1].clone()]);
        Ok(())
    }

    #[test]
    fn hard_capacity_refuses_complete_batch_then_compaction_restores_admission() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let store = Store::open_with_outbox_limits(
            &directory.path().join("tasks.sqlite"),
            TaskOutboxLimits {
                max_events: 1,
                max_bytes: 4096,
                max_consumers: 1,
            },
        )?;
        store.claim_owner(StorageOwner { id: OWNER.into() })?;
        register(&store, "observer")?;
        assert!(matches!(register(&store, "other"), Err(Error::OutboxFull)));
        assert!(matches!(
            commit(&store, vec![pending("a"), pending("b")]),
            Err(Error::OutboxFull)
        ));
        assert_eq!(store.scan(Default::default())?.sequence, 0);
        commit(&store, vec![pending("a")])?;
        assert!(matches!(
            commit(&store, vec![pending("b")]),
            Err(Error::OutboxFull)
        ));
        let page = read(&store, "observer", None, 256)?;
        ack(&store, "observer", page.next.expect("tail"))?;
        assert_eq!(compact(&store)?.removed, 1);
        commit(&store, vec![pending("b")])?;
        assert_eq!(store.scan(Default::default())?.sequence, 2);
        Ok(())
    }

    #[test]
    fn byte_capacity_and_empty_store_identity_are_explicit() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let store = Store::open_with_outbox_limits(
            &directory.path().join("tasks.sqlite"),
            TaskOutboxLimits {
                max_events: 100,
                max_bytes: 1,
                max_consumers: 1,
            },
        )?;
        store.claim_owner(StorageOwner { id: OWNER.into() })?;
        let state = register(&store, "observer")?;
        let page = read(&store, "observer", None, 256)?;
        assert_eq!(state.store_id, page.store_id);
        assert!(state.acknowledged.is_none());
        assert!(matches!(
            commit(&store, vec![pending("a")]),
            Err(Error::OutboxFull)
        ));
        assert_eq!(compact(&store)?.retained_events, 0);
        Ok(())
    }

    #[test]
    fn unsupported_fact_versions_are_refused_without_rewriting_history() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let store = open(&directory.path().join("tasks.sqlite"))?;
        commit(&store, vec![pending("current")])?;
        register(&store, "observer")?;
        let event = read(&store, "observer", None, 256)?.events.remove(0);
        let current = serde_json::to_value(&event).expect("event");
        assert_eq!(decode_event(&current.to_string())?, event);
        for version in [0, 1, 3] {
            let mut unsupported = current.clone();
            unsupported["version"] = serde_json::json!(version);
            assert!(matches!(
                decode_event(&unsupported.to_string()),
                Err(Error::Corrupt(_))
            ));
        }
        assert_eq!(read(&store, "observer", None, 256)?.events, [event]);
        Ok(())
    }

    #[test]
    #[ignore = "manual storage throughput measurement"]
    fn outbox_storage_throughput() -> Result<()> {
        for batch_size in [1, 32] {
            let directory = tempfile::tempdir()?;
            let store = open(&directory.path().join("tasks.sqlite"))?;
            register(&store, "throughput-observer")?;
            let batches = 256;
            let mut latencies = Vec::with_capacity(batches);
            let started = std::time::Instant::now();
            for batch in 0..batches {
                let writes = (0..batch_size)
                    .map(|ordinal| pending(&format!("task-{batch}-{ordinal}")))
                    .collect();
                let before = std::time::Instant::now();
                commit(&store, writes)?;
                latencies.push(before.elapsed());
            }
            let elapsed = started.elapsed();
            latencies.sort();
            let retained = compact(&store)?;
            let mut after = None;
            let mut events = 0;
            let publication = std::time::Instant::now();
            loop {
                let page = read(&store, "throughput-observer", after, 256)?;
                if page.events.is_empty() {
                    break;
                }
                events += page.events.len();
                after = page.next.clone();
                ack(
                    &store,
                    "throughput-observer",
                    page.next.expect("nonempty page tail"),
                )?;
            }
            let delivered = publication.elapsed();
            assert_eq!(events, batches * batch_size);
            println!(
                "batch_size={batch_size} commits={batches} events={events} commits_per_second={:.1} events_per_second={:.1} commit_p50_us={} commit_p95_us={} retained_json_bytes={} average_event_json_bytes={:.1} read_ack_events_per_second={:.1}",
                batches as f64 / elapsed.as_secs_f64(),
                events as f64 / elapsed.as_secs_f64(),
                latencies[batches / 2].as_micros(),
                latencies[batches * 95 / 100].as_micros(),
                retained.retained_bytes,
                retained.retained_bytes as f64 / events as f64,
                events as f64 / delivered.as_secs_f64()
            );
        }
        Ok(())
    }
}
