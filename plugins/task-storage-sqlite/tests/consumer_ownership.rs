mod common;

use std::{collections::BTreeMap, path::Path};

use swarmie_task_contracts::{
    StorageOwner, TaskOutboxAcknowledgement, TaskOutboxCompaction, TaskOutboxConsumer,
    TaskOutboxQuery, TaskRecord, TaskSpec, TaskStorage, TaskStorageInput, TaskStorageRef,
    TaskTransaction, TaskWrite,
};
use swarmie_task_storage_sqlite::Configuration;
use tembra::{core, prelude::*};

fn configuration(database: &Path) -> Configuration {
    serde_json::from_value(serde_json::json!({"database":database})).unwrap()
}

async fn open(host: &Host, configuration: Configuration) -> Result<TaskStorageRef> {
    host.install(
        swarmie_task_storage_sqlite::facet(&configuration)?,
        core::LoadOptions::default(),
    )?
    .await?;
    host.get(TaskStorage, TaskStorageInput {}).await
}

async fn seed(storage: &TaskStorageRef) -> Result<()> {
    storage
        .claim_owner(StorageOwner {
            id: "writer".into(),
        })
        .await?;
    storage
        .commit(TaskTransaction {
            owner: "writer".into(),
            writes: vec![TaskWrite {
                provenance: None,
                expected_revision: None,
                record: TaskRecord::new(
                    TaskSpec {
                        id: "task".into(),
                        kind: "test.ownership".into(),
                        definition_version: 1,
                        input: serde_json::Value::Null,
                        checkpoint: serde_json::Value::Null,
                        phase: "ready".into(),
                    },
                    None,
                ),
            }],
        })
        .await?;
    Ok(())
}

fn request(id: &str) -> TaskOutboxConsumer {
    TaskOutboxConsumer { id: id.into() }
}
fn query(id: &str) -> TaskOutboxQuery {
    TaskOutboxQuery {
        consumer_id: id.into(),
        after: None,
        limit: 256,
    }
}

#[test]
fn distinct_facets_and_unattributed_roots_cannot_take_read_ack_or_remove_another_consumer()
-> Result<()> {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("owners.sqlite");
    let driver = core::ManualDriver::new();
    driver.run_until(async {
        let host = Host::with_driver("owners", "application", driver.clone())?;
        let storage = open(&host, configuration(&database)).await?;
        seed(&storage).await?;
        let owner = common::install(&host, "consumer.owner", storage.clone()).await?;
        let other = common::install(&host, "consumer.other", storage.clone()).await?;
        let registered = owner.register_outbox_consumer(request("publisher")).await?;
        assert_eq!(registered.owner_facet.as_deref(), Some("consumer.owner"));
        let page = owner.read_outbox(query("publisher")).await?;
        let acknowledgement = TaskOutboxAcknowledgement {
            consumer_id: "publisher".into(),
            through: page.next.unwrap(),
        };
        let before = storage.outbox_status().await?;
        for error in [
            other
                .register_outbox_consumer(request("publisher"))
                .await
                .unwrap_err(),
            other.read_outbox(query("publisher")).await.unwrap_err(),
            other
                .acknowledge_outbox(acknowledgement.clone())
                .await
                .unwrap_err(),
            other
                .remove_outbox_consumer(request("publisher"))
                .await
                .unwrap_err(),
        ] {
            assert_eq!(error.code, core::ErrorCode::Forbidden);
            assert_eq!(error.reason, "task_storage_outbox_owner_mismatch");
        }
        for error in [
            storage
                .register_outbox_consumer(request("publisher"))
                .await
                .unwrap_err(),
            storage
                .register_outbox_consumer(request("unattributed-new"))
                .await
                .unwrap_err(),
            storage.read_outbox(query("publisher")).await.unwrap_err(),
            storage
                .acknowledge_outbox(acknowledgement.clone())
                .await
                .unwrap_err(),
            storage
                .remove_outbox_consumer(request("publisher"))
                .await
                .unwrap_err(),
        ] {
            assert_eq!(error.code, core::ErrorCode::Forbidden);
            assert_eq!(error.reason, "task_storage_outbox_owner_required");
        }
        assert_eq!(
            storage.outbox_status().await?,
            before,
            "all refused operations leave ownership, delivery, ack and retention intact"
        );
        let too_long = common::install(&host, &"x".repeat(257), storage.clone()).await?;
        assert_eq!(
            too_long
                .register_outbox_consumer(request("oversized-owner"))
                .await
                .unwrap_err()
                .reason,
            "task_storage_outbox_owner_invalid"
        );
        assert_eq!(storage.outbox_status().await?, before);
        owner.acknowledge_outbox(acknowledgement).await?;
        host.close().await?;
        let reopened = Host::with_driver("owners-restarted", "application", driver.clone())?;
        let storage = open(&reopened, configuration(&database)).await?;
        let owner = common::install(&reopened, "consumer.owner", storage.clone()).await?;
        let resumed = owner.register_outbox_consumer(request("publisher")).await?;
        assert_eq!(resumed.owner_facet, registered.owner_facet);
        assert!(resumed.acknowledged.is_some());
        assert!(
            owner
                .read_outbox(query("publisher"))
                .await?
                .events
                .is_empty()
        );
        owner.remove_outbox_consumer(request("publisher")).await?;
        owner.remove_outbox_consumer(request("publisher")).await?;
        reopened.close().await
    })
}

type Snapshot = (
    Vec<(String, Option<String>, u64, i64, u64, i64)>,
    Vec<(u64, u32, String)>,
);
fn snapshot(database: &Path) -> Snapshot {
    let connection = rusqlite::Connection::open(database).unwrap();
    let consumers=connection.prepare("SELECT id,owner_facet,ack_sequence,ack_ordinal,delivered_sequence,delivered_ordinal FROM task_outbox_consumers ORDER BY id").unwrap().query_map([],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?))).unwrap().collect::<std::result::Result<Vec<_>,_>>().unwrap();
    let events = connection
        .prepare("SELECT sequence,ordinal,event FROM task_outbox ORDER BY sequence,ordinal")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<std::result::Result<Vec<_>, _>>()
        .unwrap();
    (consumers, events)
}

#[test]
fn legacy_version_two_rows_preserve_retention_and_require_atomic_explicit_operator_adoption()
-> Result<()> {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("legacy.sqlite");
    let driver = core::ManualDriver::new();
    driver.run_until(async {
        let host = Host::with_driver("legacy-setup", "application", driver.clone())?;
        let storage = open(&host, configuration(&database)).await?;
        seed(&storage).await?;
        let owner = common::install(&host, "consumer.original", storage.clone()).await?;
        for id in ["legacy-a", "legacy-b"] {
            owner.register_outbox_consumer(request(id)).await?;
        }
        let original = owner.read_outbox(query("legacy-a")).await?;
        host.close().await?;
        let connection = rusqlite::Connection::open(&database).unwrap();
        connection
            .execute_batch(
                "ALTER TABLE task_outbox_consumers DROP COLUMN owner_facet; PRAGMA user_version=2;",
            )
            .unwrap();
        drop(connection);
        let host = Host::with_driver("legacy-upgrade", "application", driver.clone())?;
        let storage = open(&host, configuration(&database)).await?;
        let owner = common::install(&host, "consumer.a", storage.clone()).await?;
        let before = storage.outbox_status().await?;
        assert!(
            before
                .consumers
                .iter()
                .all(|consumer| consumer.owner_facet.is_none())
        );
        assert_eq!(before.pending_events, 1);
        assert_eq!(
            storage
                .compact_outbox(TaskOutboxCompaction { limit: 4096 })
                .await?
                .removed,
            0
        );
        for error in [
            owner
                .register_outbox_consumer(request("legacy-a"))
                .await
                .unwrap_err(),
            owner.read_outbox(query("legacy-a")).await.unwrap_err(),
            owner
                .acknowledge_outbox(TaskOutboxAcknowledgement {
                    consumer_id: "legacy-a".into(),
                    through: original.events[0].id.clone(),
                })
                .await
                .unwrap_err(),
            owner
                .remove_outbox_consumer(request("legacy-a"))
                .await
                .unwrap_err(),
        ] {
            assert_eq!(error.reason, "task_storage_outbox_owner_unassigned");
            assert_eq!(error.code, core::ErrorCode::UpgradeRequired);
        }
        assert_eq!(storage.outbox_status().await?, before);
        host.close().await?;
        let persisted = snapshot(&database);
        let mut bad = configuration(&database);
        bad.outbox_legacy_owners = BTreeMap::from([
            ("legacy-a".into(), "consumer.a".into()),
            ("missing-z".into(), "consumer.a".into()),
        ]);
        let failed = Host::with_driver("legacy-adoption-refused", "application", driver.clone())?;
        assert_eq!(
            open(&failed, bad).await.err().unwrap().reason,
            "task_storage_invalid"
        );
        failed.close().await?;
        assert_eq!(
            snapshot(&database),
            persisted,
            "a valid first mapping cannot partially adopt before a later refused mapping"
        );
        let mut adopted = configuration(&database);
        adopted.outbox_legacy_owners = BTreeMap::from([
            ("legacy-a".into(), "consumer.a".into()),
            ("legacy-b".into(), "consumer.b".into()),
        ]);
        let host = Host::with_driver("legacy-adopted", "application", driver.clone())?;
        let storage = open(&host, adopted.clone()).await?;
        let a = common::install(&host, "consumer.a", storage.clone()).await?;
        let b = common::install(&host, "consumer.b", storage.clone()).await?;
        assert_eq!(
            a.register_outbox_consumer(request("legacy-a"))
                .await?
                .owner_facet
                .as_deref(),
            Some("consumer.a")
        );
        assert_eq!(
            b.register_outbox_consumer(request("legacy-b"))
                .await?
                .owner_facet
                .as_deref(),
            Some("consumer.b")
        );
        assert_eq!(
            a.read_outbox(query("legacy-a")).await?.events,
            original.events
        );
        assert_eq!(
            b.read_outbox(query("legacy-b")).await?.events,
            original.events
        );
        assert_eq!(
            a.read_outbox(query("legacy-b")).await.unwrap_err().reason,
            "task_storage_outbox_owner_mismatch"
        );
        host.close().await?;
        let persisted = snapshot(&database);
        let mut conflict = configuration(&database);
        conflict.outbox_legacy_owners = BTreeMap::from([("legacy-a".into(), "consumer.b".into())]);
        let failed = Host::with_driver("legacy-no-reassignment", "application", driver.clone())?;
        assert_eq!(
            open(&failed, conflict).await.err().unwrap().reason,
            "task_storage_outbox_owner_mismatch"
        );
        failed.close().await?;
        assert_eq!(snapshot(&database), persisted);
        let host = Host::with_driver("legacy-adoption-repeated", "application", driver.clone())?;
        let storage = open(&host, adopted).await?;
        assert_eq!(storage.outbox_status().await?.pending_events, 1);
        host.close().await
    })
}
