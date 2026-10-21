//! Deliberately ephemeral replacement fixture. Root tasks only; owned work is explicitly refused.
//! This provider proves Profile substitution, and does not claim backend conformance or durability.
//! Its atomic outbox retains at most 4096 events or 16 MiB and 64 consumers. Pending events
//! never expire; capacity rejects the whole commit. Resource closure discards all fixture data.

use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
};

use futures::FutureExt;
use swarmie_task_contracts::*;
use tembra::{core, prelude::*};

pub const PLUGIN_ID: &str = "fixture.volatile-task-storage";
pub const FACET_ID: &str = "fixture.volatile-task-storage.provider";

struct Data {
    store_id: String,
    owner: Option<String>,
    records: BTreeMap<String, TaskRecord>,
    sequence: u64,
    outbox: crate::memory_outbox::MemoryOutbox,
}

impl Default for Data {
    fn default() -> Self {
        Self {
            store_id: uuid::Uuid::new_v4().to_string(),
            owner: None,
            records: BTreeMap::new(),
            sequence: 0,
            outbox: Default::default(),
        }
    }
}

pub fn facet() -> Result<NativeFacet> {
    NativeFacet::new(FACET_ID, vec![], |_, setup| {
        setup.serve_with_options(
            TaskStorage,
            |_, _, declaration| {
                let resource = declaration.core_mut().resource(
                    "fixture-records",
                    ResourceDeclaration::new(|_| {
                        async { Ok(RefCell::new(Data::default())) }.boxed_local()
                    })
                    .with_close(|value| {
                        async move {
                            *value.borrow_mut() = Data::default();
                            Ok(())
                        }
                        .boxed_local()
                    }),
                )?;
                Ok(Service { resource })
            },
            core::ServeOptions {
                lifetime: core::Lifetime::Host,
                ..Default::default()
            },
        )
    })
}

struct Service {
    resource: Resource<RefCell<Data>>,
}

fn invalid(message: &str) -> RuntimeError {
    RuntimeError::invalid("fixture_storage_unsupported", message)
}

impl TaskStorageProvider for Service {
    async fn outbox_status(&self, _context: Context) -> Result<TaskOutboxStatus> {
        let value = self.resource.value()?;
        let data = value.borrow();
        Ok(data.outbox.status(&data.store_id))
    }

    async fn claim_owner(&self, owner: StorageOwner, _context: Context) -> Result<()> {
        validate_identity(&owner.id)?;
        let value = self.resource.value()?;
        let mut data = value.borrow_mut();
        if data
            .owner
            .as_ref()
            .is_some_and(|actual| actual != &owner.id)
        {
            return Err(invalid("another fixture owner is active"));
        }
        data.owner = Some(owner.id);
        Ok(())
    }
    async fn release_owner(&self, owner: StorageOwner, _context: Context) -> Result<()> {
        let value = self.resource.value()?;
        let mut data = value.borrow_mut();
        if data.owner.as_deref() != Some(&owner.id) {
            return Err(invalid("fixture owner does not match"));
        }
        data.owner = None;
        Ok(())
    }
    async fn get(&self, request: TaskLookup, _context: Context) -> Result<Option<TaskRecord>> {
        validate_identity(&request.id)?;
        Ok(self
            .resource
            .value()?
            .borrow()
            .records
            .get(&request.id)
            .cloned())
    }
    async fn scan(&self, query: TaskQuery, _context: Context) -> Result<TaskPage> {
        if !(1..=256).contains(&query.limit) {
            return Err(invalid("fixture query limit must be 1..=256"));
        }
        let value = self.resource.value()?;
        let data = value.borrow();
        if let Some(cursor) = &query.cursor
            && (cursor.store_id != data.store_id
                || cursor.sequence != data.sequence
                || cursor.parent != query.parent
                || cursor.statuses != query.statuses)
        {
            return Err(RuntimeError::invalid(
                "task_storage_cursor_invalidated",
                "fixture cursor does not match committed snapshot",
            ));
        }
        let after = query.cursor.as_ref().map(|cursor| cursor.after.as_str());
        let mut selected = data
            .records
            .values()
            .filter(|record| {
                after.is_none_or(|id| record.id.as_str() > id)
                    && (query.statuses.is_empty() || query.statuses.contains(&record.status))
                    && query
                        .parent
                        .as_ref()
                        .is_none_or(|id| record.parent.as_ref() == Some(id))
            })
            .take(query.limit as usize + 1)
            .cloned()
            .collect::<Vec<_>>();
        let more = selected.len() > query.limit as usize;
        if more {
            selected.pop();
        }
        let next = if more {
            Some(TaskCursor {
                store_id: data.store_id.clone(),
                sequence: data.sequence,
                after: selected.last().unwrap().id.clone(),
                statuses: query.statuses.clone(),
                parent: query.parent.clone(),
            })
        } else {
            None
        };
        let mut page = TaskPage {
            sequence: data.sequence,
            records: selected,
            next,
        };
        while let Err(error) = validate_task_payload_size(&page) {
            if error.reason != "task_storage_too_large" || page.records.len() <= 1 {
                return Err(error);
            }
            page.records.pop();
            page.next = Some(TaskCursor {
                store_id: data.store_id.clone(),
                sequence: data.sequence,
                after: page.records.last().unwrap().id.clone(),
                statuses: query.statuses.clone(),
                parent: query.parent.clone(),
            });
        }
        Ok(page)
    }
    async fn commit(
        &self,
        transaction: TaskTransaction,
        _context: Context,
    ) -> Result<CommitResult> {
        let value = self.resource.value()?;
        let mut data = value.borrow_mut();
        if data.owner.as_deref() != Some(&transaction.owner) {
            return Err(invalid("fixture transaction requires its active owner"));
        }
        if transaction.writes.is_empty() || transaction.writes.len() > 256 {
            return Err(invalid("fixture transaction requires 1..=256 writes"));
        }
        validate_task_payload_size(&transaction)?;
        let portable = tembra::contracts::to_portable_value(&transaction)
            .map_err(|error| invalid(&error.to_string()))?;
        let transaction: TaskTransaction = tembra::contracts::from_portable_value(&portable)
            .map_err(|error| invalid(&error.to_string()))?;
        let mut ids = BTreeSet::new();
        for write in &transaction.writes {
            let record = &write.record;
            validate_task_record_size(record)?;
            validate_identity(&record.id)?;
            validate_identity(&record.kind)?;
            validate_identity(&record.phase)?;
            if !ids.insert(record.id.clone()) {
                return Err(invalid("duplicate fixture write identity"));
            }
            if record.parent.is_some()
                || !record.wait_for.is_empty()
                || record.status == TaskStatus::Waiting
            {
                return Err(invalid(
                    "fixture replacement intentionally supports root tasks only",
                ));
            }
            let old = data.records.get(&record.id);
            if old.map(|record| record.revision) != write.expected_revision {
                return Ok(CommitResult::Conflict {
                    id: record.id.clone(),
                    actual_revision: old.map(|record| record.revision),
                });
            }
            let expected = write
                .expected_revision
                .unwrap_or(0)
                .checked_add(1)
                .ok_or_else(|| invalid("revision overflow"))?;
            if record.revision != expected {
                return Err(invalid("fixture revisions must increment by one"));
            }
            if old.is_some_and(|old| {
                old.is_terminal() || old.kind != record.kind || old.input != record.input
            }) {
                return Err(invalid("immutable fixture record changed"));
            }
            if record.is_terminal() != record.outcome.is_some()
                && record.status != TaskStatus::Completing
            {
                return Err(invalid("fixture outcome does not match status"));
            }
        }
        let sequence = data
            .sequence
            .checked_add(1)
            .filter(|sequence| *sequence <= MAX_PORTABLE_INTEGER)
            .ok_or_else(|| invalid("sequence exhausted"))?;
        let events = data
            .outbox
            .prepare(&transaction, &data.records, &data.store_id, sequence)?;
        let records = transaction
            .writes
            .into_iter()
            .map(|write| write.record)
            .collect::<Vec<_>>();
        let receipt = CommitResult::Committed {
            sequence,
            records: records.clone(),
        };
        validate_task_payload_size(&receipt)?;
        for record in &records {
            data.records.insert(record.id.clone(), record.clone());
        }
        data.outbox.append(events);
        data.sequence = sequence;
        Ok(receipt)
    }

    async fn register_outbox_consumer(
        &self,
        consumer: TaskOutboxConsumer,
        context: Context,
    ) -> Result<TaskOutboxConsumerState> {
        let owner = task_outbox_consumer_owner(&context)?;
        let value = self.resource.value()?;
        let mut data = value.borrow_mut();
        let store_id = data.store_id.clone();
        data.outbox.register(consumer, &store_id, &owner)
    }

    async fn read_outbox(
        &self,
        query: TaskOutboxQuery,
        context: Context,
    ) -> Result<TaskOutboxPage> {
        let owner = task_outbox_consumer_owner(&context)?;
        let value = self.resource.value()?;
        let mut data = value.borrow_mut();
        let store_id = data.store_id.clone();
        data.outbox.read(query, &store_id, &owner)
    }

    async fn acknowledge_outbox(
        &self,
        acknowledgement: TaskOutboxAcknowledgement,
        context: Context,
    ) -> Result<()> {
        let owner = task_outbox_consumer_owner(&context)?;
        let value = self.resource.value()?;
        let mut data = value.borrow_mut();
        let store_id = data.store_id.clone();
        data.outbox.acknowledge(acknowledgement, &store_id, &owner)
    }

    async fn remove_outbox_consumer(
        &self,
        consumer: TaskOutboxConsumer,
        context: Context,
    ) -> Result<()> {
        let owner = task_outbox_consumer_owner(&context)?;
        self.resource
            .value()?
            .borrow_mut()
            .outbox
            .remove(consumer, &owner)
    }

    async fn compact_outbox(
        &self,
        request: TaskOutboxCompaction,
        _context: Context,
    ) -> Result<TaskOutboxRetention> {
        self.resource.value()?.borrow_mut().outbox.compact(request)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(id: &str) -> TaskWrite {
        TaskWrite {
            provenance: None,
            expected_revision: None,
            record: TaskRecord::new(crate::task(id), None),
        }
    }

    #[test]
    fn independent_provider_outbox_preserves_atomicity_replay_and_retention() {
        let driver = tembra_native::TokioDriver::new();
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(driver.run_until(async {
                let host = Host::with_driver("volatile-outbox", "test", driver.clone()).unwrap();
                host.install(facet().unwrap(), core::LoadOptions::default())
                    .unwrap()
                    .await
                    .unwrap();
                let storage = host.get(TaskStorage, TaskStorageInput {}).await.unwrap();
                storage
                    .claim_owner(StorageOwner { id: "owner".into() })
                    .await
                    .unwrap();
                let consumer = crate::outbox_consumer::install(
                    &host,
                    "fixture.memory-consumer",
                    storage.clone(),
                )
                .await
                .unwrap();
                let register = |id: &str| TaskOutboxConsumer { id: id.into() };
                let empty = storage.outbox_status().await.unwrap();
                assert_eq!(empty.retained_events, 0);
                assert_eq!(empty.limits.max_events, 4096);
                assert_eq!(storage.outbox_status().await.unwrap(), empty);
                let initial = consumer
                    .register_outbox_consumer(register("publisher"))
                    .await
                    .unwrap();
                assert!(uuid::Uuid::parse_str(&initial.store_id).is_ok());
                consumer
                    .register_outbox_consumer(register("offline"))
                    .await
                    .unwrap();
                let transaction = TaskTransaction {
                    owner: "owner".into(),
                    writes: vec![write("z-first"), write("a-second")],
                };
                assert!(matches!(
                    storage.commit(transaction.clone()).await.unwrap(),
                    CommitResult::Committed { sequence: 1, .. }
                ));
                assert!(matches!(
                    storage.commit(transaction).await.unwrap(),
                    CommitResult::Conflict { .. }
                ));
                let query = |id: &str, after, limit| TaskOutboxQuery {
                    consumer_id: id.into(),
                    after,
                    limit,
                };
                let pending = storage.outbox_status().await.unwrap();
                assert_eq!(pending.retained_events, 2);
                assert_eq!(pending.pending_events, 2);
                assert_eq!(pending.compactable_events, 0);
                assert_eq!(pending.consumers.len(), 2);
                let first = consumer
                    .read_outbox(query("publisher", None, 1))
                    .await
                    .unwrap();
                assert_eq!(first.events.len(), 1);
                assert_eq!(first.events[0].task_id, "z-first");
                assert_eq!(first.events[0].id.ordinal, 0);
                assert_eq!(first.events[0].transaction_size, 2);
                let second = consumer
                    .read_outbox(query("publisher", first.next.clone(), 1))
                    .await
                    .unwrap();
                assert_eq!(second.events[0].task_id, "a-second");
                assert_eq!(second.events[0].id.ordinal, 1);
                assert!(
                    consumer
                        .read_outbox(query("publisher", second.next.clone(), 256))
                        .await
                        .unwrap()
                        .events
                        .is_empty()
                );
                // Publication without acknowledgement repeats the stable identity on resume.
                let replay = consumer
                    .read_outbox(query("publisher", None, 256))
                    .await
                    .unwrap();
                assert_eq!(replay.events[0].id, first.events[0].id);
                consumer
                    .acknowledge_outbox(TaskOutboxAcknowledgement {
                        consumer_id: "publisher".into(),
                        through: second.next.clone().unwrap(),
                    })
                    .await
                    .unwrap();
                assert_eq!(
                    storage
                        .compact_outbox(TaskOutboxCompaction { limit: 4096 })
                        .await
                        .unwrap()
                        .removed,
                    0
                );
                let recovered = consumer
                    .read_outbox(query("offline", None, 256))
                    .await
                    .unwrap();
                assert_eq!(recovered.events, replay.events);
                consumer
                    .remove_outbox_consumer(register("offline"))
                    .await
                    .unwrap();
                assert_eq!(
                    storage
                        .compact_outbox(TaskOutboxCompaction { limit: 1 })
                        .await
                        .unwrap()
                        .removed,
                    1
                );
                let late = consumer
                    .register_outbox_consumer(register("late"))
                    .await
                    .unwrap();
                assert_eq!(late.retained_after, first.next);
                assert_eq!(
                    consumer
                        .read_outbox(query("late", None, 256))
                        .await
                        .unwrap()
                        .events[0]
                        .id,
                    second.events[0].id
                );
                storage
                    .commit(TaskTransaction {
                        owner: "owner".into(),
                        writes: vec![write("appended")],
                    })
                    .await
                    .unwrap();
                assert_eq!(
                    consumer
                        .read_outbox(query("publisher", second.next, 256))
                        .await
                        .unwrap()
                        .events[0]
                        .task_id,
                    "appended"
                );
                let mut foreign = first.events[0].id.clone();
                foreign.store_id = uuid::Uuid::new_v4().to_string();
                assert!(
                    consumer
                        .acknowledge_outbox(TaskOutboxAcknowledgement {
                            consumer_id: "publisher".into(),
                            through: foreign
                        })
                        .await
                        .is_err()
                );
                host.close().await.unwrap();
            }));
    }

    #[test]
    fn independent_provider_full_outbox_rejects_record_and_sequence_atomically() {
        let driver = tembra_native::TokioDriver::new();
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(driver.run_until(async {
                let host = Host::with_driver("volatile-capacity", "test", driver.clone()).unwrap();
                host.install(facet().unwrap(), core::LoadOptions::default())
                    .unwrap()
                    .await
                    .unwrap();
                let storage = host.get(TaskStorage, TaskStorageInput {}).await.unwrap();
                storage
                    .claim_owner(StorageOwner { id: "owner".into() })
                    .await
                    .unwrap();
                for batch in 0..16 {
                    let writes = (0..256)
                        .map(|ordinal| write(&format!("task-{batch}-{ordinal}")))
                        .collect();
                    storage
                        .commit(TaskTransaction {
                            owner: "owner".into(),
                            writes,
                        })
                        .await
                        .unwrap();
                }
                let before = storage.outbox_status().await.unwrap();
                assert_eq!(before.retained_events, 4096);
                assert_eq!(before.pending_events, 4096);
                assert!(before.consumers.is_empty());
                let error = storage
                    .commit(TaskTransaction {
                        owner: "owner".into(),
                        writes: vec![write("rejected")],
                    })
                    .await
                    .unwrap_err();
                assert_eq!(error.reason, "task_storage_outbox_full");
                assert_eq!(storage.outbox_status().await.unwrap(), before);
                assert_eq!(error.code, tembra::ErrorCode::Unavailable);
                assert!(
                    storage
                        .get(TaskLookup {
                            id: "rejected".into()
                        })
                        .await
                        .unwrap()
                        .is_none()
                );
                assert_eq!(
                    storage
                        .scan(TaskQuery {
                            limit: 1,
                            ..Default::default()
                        })
                        .await
                        .unwrap()
                        .sequence,
                    16
                );
                let retention = storage
                    .compact_outbox(TaskOutboxCompaction { limit: 4096 })
                    .await
                    .unwrap();
                assert_eq!(retention.removed, 0);
                assert_eq!(retention.retained_events, 4096);
                host.close().await.unwrap();
            }));
    }

    #[test]
    fn independent_provider_refuses_foreign_and_unattributed_consumer_mutations() {
        let driver = tembra_native::TokioDriver::new();
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(driver.run_until(async {
                let host =
                    Host::with_driver("volatile-owned-consumers", "test", driver.clone()).unwrap();
                host.install(facet().unwrap(), core::LoadOptions::default())
                    .unwrap()
                    .await
                    .unwrap();
                let storage = host.get(TaskStorage, TaskStorageInput {}).await.unwrap();
                storage
                    .claim_owner(StorageOwner { id: "owner".into() })
                    .await
                    .unwrap();
                storage
                    .commit(TaskTransaction {
                        owner: "owner".into(),
                        writes: vec![write("owned")],
                    })
                    .await
                    .unwrap();
                let a =
                    crate::outbox_consumer::install(&host, "fixture.consumer.a", storage.clone())
                        .await
                        .unwrap();
                let b =
                    crate::outbox_consumer::install(&host, "fixture.consumer.b", storage.clone())
                        .await
                        .unwrap();
                let request = || TaskOutboxConsumer {
                    id: "publisher".into(),
                };
                let query = || TaskOutboxQuery {
                    consumer_id: "publisher".into(),
                    after: None,
                    limit: 256,
                };
                assert_eq!(
                    a.register_outbox_consumer(request())
                        .await
                        .unwrap()
                        .owner_facet
                        .as_deref(),
                    Some("fixture.consumer.a")
                );
                let page = a.read_outbox(query()).await.unwrap();
                let acknowledgement = TaskOutboxAcknowledgement {
                    consumer_id: "publisher".into(),
                    through: page.next.unwrap(),
                };
                let before = storage.outbox_status().await.unwrap();
                for error in [
                    b.register_outbox_consumer(request()).await.unwrap_err(),
                    b.read_outbox(query()).await.unwrap_err(),
                    b.acknowledge_outbox(acknowledgement.clone())
                        .await
                        .unwrap_err(),
                    b.remove_outbox_consumer(request()).await.unwrap_err(),
                ] {
                    assert_eq!(error.reason, "task_storage_outbox_owner_mismatch");
                }
                for error in [
                    storage
                        .register_outbox_consumer(request())
                        .await
                        .unwrap_err(),
                    storage.read_outbox(query()).await.unwrap_err(),
                    storage
                        .acknowledge_outbox(acknowledgement.clone())
                        .await
                        .unwrap_err(),
                    storage.remove_outbox_consumer(request()).await.unwrap_err(),
                ] {
                    assert_eq!(error.reason, "task_storage_outbox_owner_required");
                }
                assert_eq!(storage.outbox_status().await.unwrap(), before);
                a.acknowledge_outbox(acknowledgement).await.unwrap();
                host.close().await.unwrap();
            }));
    }
}
