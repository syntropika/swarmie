mod common;

use std::path::Path;

use swarmie_task_contracts::{
    StorageOwner, TaskOutboxAcknowledgement, TaskOutboxConsumer, TaskOutboxQuery, TaskRecord,
    TaskSpec, TaskStorage, TaskStorageInput, TaskStorageRef, TaskTransaction, TaskWrite,
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
        assert_eq!(registered.owner_facet, "consumer.owner");
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
