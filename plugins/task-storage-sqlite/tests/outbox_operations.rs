use std::path::Path;

use swarmie_task_contracts::{
    CommitResult, StorageOwner, TaskOutboxAcknowledgement, TaskOutboxCompaction,
    TaskOutboxConsumer, TaskOutboxLimits, TaskOutboxQuery, TaskRecord, TaskSpec, TaskStorage,
    TaskStorageInput, TaskStorageRef, TaskTransaction, TaskWrite,
};
use swarmie_task_storage_sqlite::{Configuration, ConfigurationError};
use tembra::{core, prelude::*};

async fn open(host: &Host, configuration: Configuration) -> Result<TaskStorageRef> {
    host.install(
        swarmie_task_storage_sqlite::facet(&configuration)?,
        core::LoadOptions::default(),
    )?
    .await?;
    host.get(TaskStorage, TaskStorageInput {}).await
}

fn configuration(database: &Path, events: u64) -> Configuration {
    serde_json::from_value(serde_json::json!({
        "database":database,
        "outbox":{"max_events":events,"max_bytes":65_536}
    }))
    .expect("operator JSON configuration")
}

async fn commit(storage: &TaskStorageRef, id: &str) -> Result<CommitResult> {
    storage
        .commit(TaskTransaction {
            owner: "operations-owner".into(),
            writes: vec![TaskWrite {
                expected_revision: None,
                record: TaskRecord::new(
                    TaskSpec {
                        id: id.into(),
                        kind: "test.operations".into(),
                        definition_version: 1,
                        input: serde_json::Value::Null,
                        checkpoint: serde_json::Value::Null,
                        phase: "ready".into(),
                    },
                    None,
                ),
            }],
        })
        .await
}

#[test]
fn limits_json_defaults_partial_values_and_validation_are_explicit() {
    let default: Configuration =
        serde_json::from_value(serde_json::json!({"database":"tasks.sqlite"})).unwrap();
    assert_eq!(default.outbox, TaskOutboxLimits::default());
    for outbox in [
        serde_json::json!({}),
        serde_json::json!({"max_events":3}),
        serde_json::json!({"max_bytes":8192}),
        serde_json::json!({"max_consumers":2}),
    ] {
        let parsed: Configuration =
            serde_json::from_value(serde_json::json!({"database":"tasks.sqlite","outbox":outbox}))
                .unwrap();
        parsed.validate().unwrap();
        assert_eq!(
            parsed.outbox.max_events,
            outbox["max_events"].as_u64().unwrap_or(1_000_000)
        );
        assert_eq!(
            parsed.outbox.max_bytes,
            outbox["max_bytes"].as_u64().unwrap_or(1_073_741_824)
        );
        assert_eq!(
            parsed.outbox.max_consumers,
            u32::try_from(outbox["max_consumers"].as_u64().unwrap_or(64)).unwrap()
        );
    }
    for outbox in [
        serde_json::json!({"max_events":0}),
        serde_json::json!({"max_bytes":0}),
        serde_json::json!({"max_consumers":0}),
        serde_json::json!({"max_consumers":65}),
        serde_json::json!({"max_events":swarmie_task_contracts::MAX_PORTABLE_INTEGER+1}),
        serde_json::json!({"max_bytes":swarmie_task_contracts::MAX_PORTABLE_INTEGER+1}),
    ] {
        let parsed: Configuration =
            serde_json::from_value(serde_json::json!({"database":"tasks.sqlite","outbox":outbox}))
                .unwrap();
        assert_eq!(parsed.validate(), Err(ConfigurationError::OutboxLimits));
        assert!(swarmie_task_storage_sqlite::facet(&parsed).is_err());
    }
    for outbox in [
        serde_json::json!({"max_events":-1}),
        serde_json::json!({"max_bytes":"large"}),
        serde_json::json!({"ttl":1}),
        serde_json::Value::Null,
    ] {
        assert!(
            serde_json::from_value::<Configuration>(
                serde_json::json!({"database":"tasks.sqlite","outbox":outbox})
            )
            .is_err()
        );
    }
}

#[test]
fn no_consumers_full_capacity_remains_visible_and_larger_json_limits_resume_writes() -> Result<()> {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("operations.sqlite");
    let driver = core::ManualDriver::new();
    driver.run_until(async {
        let host = Host::with_driver("outbox-operations", "application", driver.clone())?;
        let storage = open(&host, configuration(&database, 2)).await?;
        let empty = storage.outbox_status().await?;
        assert_eq!(empty.limits.max_events, 2);
        assert_eq!(empty.retained_events, 0);
        assert_eq!(empty.pending_events, 0);
        assert!(empty.consumers.is_empty());
        storage
            .claim_owner(StorageOwner {
                id: "operations-owner".into(),
            })
            .await?;
        commit(&storage, "first").await?;
        commit(&storage, "second").await?;
        assert_eq!(
            commit(&storage, "rejected").await.unwrap_err().reason,
            "task_storage_outbox_full"
        );
        storage
            .release_owner(StorageOwner {
                id: "operations-owner".into(),
            })
            .await?;
        let full = storage.outbox_status().await?;
        assert_eq!(full.retained_events, 2);
        assert!(full.retained_bytes > 0);
        assert_eq!(full.pending_events, 2);
        assert_eq!(full.compactable_events, 0);
        assert!(full.consumers.is_empty());
        let raw = rusqlite::Connection::open(&database).unwrap();
        let before: u64 = raw
            .pragma_query_value(None, "data_version", |row| row.get(0))
            .unwrap();
        assert_eq!(storage.outbox_status().await?, full);
        assert_eq!(storage.outbox_status().await?, full);
        let after: u64 = raw
            .pragma_query_value(None, "data_version", |row| row.get(0))
            .unwrap();
        assert_eq!(before, after, "monitoring must not mutate SQLite");
        assert_eq!(
            storage
                .compact_outbox(TaskOutboxCompaction { limit: 4096 })
                .await?
                .removed,
            0
        );
        host.close().await?;
        let larger = Host::with_driver("outbox-larger-limits", "application", driver.clone())?;
        let storage = open(&larger, configuration(&database, 4)).await?;
        let resumed = storage.outbox_status().await?;
        assert_eq!(resumed.store_id, full.store_id);
        assert_eq!(resumed.retained_events, full.retained_events);
        assert_eq!(resumed.retained_bytes, full.retained_bytes);
        assert_eq!(resumed.limits.max_events, 4);
        storage
            .claim_owner(StorageOwner {
                id: "operations-owner".into(),
            })
            .await?;
        commit(&storage, "third").await?;
        storage
            .register_outbox_consumer(TaskOutboxConsumer {
                id: "publisher".into(),
            })
            .await?;
        let page = storage
            .read_outbox(TaskOutboxQuery {
                consumer_id: "publisher".into(),
                after: None,
                limit: 256,
            })
            .await?;
        assert_eq!(
            page.events
                .iter()
                .map(|event| event.task_id.as_str())
                .collect::<Vec<_>>(),
            ["first", "second", "third"]
        );
        assert_eq!(
            page.events
                .iter()
                .map(|event| event.id.sequence)
                .collect::<Vec<_>>(),
            [1, 2, 3]
        );
        larger.close().await
    })
}

#[test]
fn offline_consumer_progress_and_explicit_retention_are_visible_without_scheduler() -> Result<()> {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("progress.sqlite");
    let driver = core::ManualDriver::new();
    driver.run_until(async {
        let host = Host::with_driver("outbox-progress", "application", driver.clone())?;
        let storage = open(&host, configuration(&database, 4)).await?;
        for id in ["z-online", "a-offline"] {
            storage
                .register_outbox_consumer(TaskOutboxConsumer { id: id.into() })
                .await?;
        }
        storage
            .claim_owner(StorageOwner {
                id: "operations-owner".into(),
            })
            .await?;
        commit(&storage, "first").await?;
        commit(&storage, "second").await?;
        let initial = storage.outbox_status().await?;
        assert_eq!(
            initial
                .consumers
                .iter()
                .map(|consumer| consumer.consumer_id.as_str())
                .collect::<Vec<_>>(),
            ["a-offline", "z-online"]
        );
        let page = storage
            .read_outbox(TaskOutboxQuery {
                consumer_id: "z-online".into(),
                after: None,
                limit: 256,
            })
            .await?;
        storage
            .acknowledge_outbox(TaskOutboxAcknowledgement {
                consumer_id: "z-online".into(),
                through: page.next.clone().unwrap(),
            })
            .await?;
        let lagging = storage.outbox_status().await?;
        assert_eq!(lagging.pending_events, 2);
        assert_eq!(lagging.compactable_events, 0);
        assert!(lagging.consumers[0].acknowledged.is_none());
        assert_eq!(lagging.consumers[1].acknowledged, page.next);
        assert_eq!(lagging.consumers[1].delivered, page.next);
        let replay = storage
            .read_outbox(TaskOutboxQuery {
                consumer_id: "a-offline".into(),
                after: None,
                limit: 256,
            })
            .await?;
        storage
            .acknowledge_outbox(TaskOutboxAcknowledgement {
                consumer_id: "a-offline".into(),
                through: replay.events[0].id.clone(),
            })
            .await?;
        let partial = storage.outbox_status().await?;
        assert_eq!(partial.retained_events, 2);
        assert_eq!(partial.compactable_events, 1);
        assert_eq!(partial.pending_events, 1);
        let bytes = partial.retained_bytes;
        assert_eq!(
            storage
                .compact_outbox(TaskOutboxCompaction { limit: 1 })
                .await?
                .removed,
            1
        );
        let compacted = storage.outbox_status().await?;
        assert_eq!(compacted.retained_events, 1);
        assert!(compacted.retained_bytes < bytes);
        assert_eq!(compacted.compactable_events, 0);
        assert_eq!(compacted.pending_events, 1);
        assert_eq!(compacted.retained_after, Some(replay.events[0].id.clone()));
        storage
            .acknowledge_outbox(TaskOutboxAcknowledgement {
                consumer_id: "a-offline".into(),
                through: replay.next.unwrap(),
            })
            .await?;
        let ready = storage.outbox_status().await?;
        assert_eq!(ready.compactable_events, 1);
        assert_eq!(ready.pending_events, 0);
        for id in ["z-online", "a-offline"] {
            storage
                .remove_outbox_consumer(TaskOutboxConsumer { id: id.into() })
                .await?;
        }
        let unowned = storage.outbox_status().await?;
        assert_eq!(unowned.retained_events, 1);
        assert_eq!(unowned.compactable_events, 0);
        assert_eq!(unowned.pending_events, 1);
        assert!(unowned.consumers.is_empty());
        assert_eq!(
            storage
                .compact_outbox(TaskOutboxCompaction { limit: 4096 })
                .await?
                .removed,
            0
        );
        host.close().await
    })
}
