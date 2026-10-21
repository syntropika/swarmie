use std::path::Path;

use swarmie_task_contracts::{
    StorageOwner, TaskLookup, TaskStorage, TaskStorageInput, TaskStorageRef,
};
use swarmie_task_storage_sqlite::{Configuration, ConfigurationError};
use tembra::{core, prelude::*};

async fn open(host: &Host, database: &Path) -> Result<TaskStorageRef> {
    host.install(
        swarmie_task_storage_sqlite::facet(&Configuration {
            database: database.into(),
            outbox: Default::default(),
        })?,
        core::LoadOptions::default(),
    )?
    .await?;
    host.get(TaskStorage, TaskStorageInput {}).await
}

#[test]
fn public_contract_backend_conformance_and_restart_persistence() -> Result<()> {
    let directory = tempfile::tempdir().expect("temporary database directory");
    let database = directory.path().join("tasks.sqlite");
    let driver = core::ManualDriver::new();
    let host = Host::with_driver("storage-conformance", "application", driver.clone())?;
    driver.run_until(async {
        let storage = open(&host, &database).await?;
        swarmie_task_contracts::conformance::run(&storage).await?;
        let before = storage
            .get(TaskLookup {
                id: "parent".into(),
            })
            .await?
            .expect("parent receipt");
        host.close().await?;
        assert!(
            storage
                .get(TaskLookup {
                    id: "parent".into()
                })
                .await
                .is_err()
        );
        let next = Host::with_driver("storage-reopened", "application", driver.clone())?;
        let reopened = open(&next, &database).await?;
        assert_eq!(
            reopened
                .get(TaskLookup {
                    id: "parent".into()
                })
                .await?,
            Some(before)
        );
        let large = reopened
            .get(TaskLookup {
                id: "large-a".into(),
            })
            .await?
            .expect("large checkpoint survives restart");
        assert_eq!(large.checkpoint["data"].as_str().unwrap().len(), 600_000);
        let terminal = reopened
            .get(TaskLookup {
                id: "control-reserve".into(),
            })
            .await?
            .expect("retained terminal checkpoint survives restart");
        assert!(terminal.is_terminal());
        assert_eq!(terminal.attempts, 10);
        assert_eq!(terminal.restarts, 10);
        assert!(terminal.outcome.is_some());
        reopened
            .claim_owner(StorageOwner {
                id: "new-process-owner".into(),
            })
            .await?;
        next.close().await
    })
}

#[test]
fn resource_lock_excludes_other_hosts_and_releases_at_close() -> Result<()> {
    let directory = tempfile::tempdir().expect("temporary database directory");
    let database = directory.path().join("tasks.sqlite");
    let driver = core::ManualDriver::new();
    let first = Host::with_driver("storage-first", "application", driver.clone())?;
    driver.run_until(async {
        let _storage = open(&first, &database).await?;
        let second = Host::with_driver("storage-second", "application", driver.clone())?;
        assert_eq!(
            open(&second, &database)
                .await
                .err()
                .expect("database owner refusal")
                .reason,
            "task_storage_owned"
        );
        second.close().await?;
        first.close().await?;
        let third = Host::with_driver("storage-successor", "application", driver.clone())?;
        let _storage = open(&third, &database).await?;
        third.close().await
    })
}

#[cfg(unix)]
#[test]
fn symlinks_share_ownership_and_hard_links_are_refused() -> Result<()> {
    use std::os::unix::fs::symlink;
    let directory = tempfile::tempdir().expect("temporary database directory");
    let database = directory.path().join("tasks.sqlite");
    let alias = directory.path().join("alias.sqlite");
    let driver = core::ManualDriver::new();
    let first = Host::with_driver("storage-first", "application", driver.clone())?;
    driver.run_until(async {
        let _storage = open(&first, &database).await?;
        symlink(&database, &alias).expect("database alias");
        let second = Host::with_driver("storage-alias", "application", driver.clone())?;
        assert_eq!(
            open(&second, &alias)
                .await
                .err()
                .expect("alias owner refusal")
                .reason,
            "task_storage_owned"
        );
        second.close().await?;
        first.close().await?;
        std::fs::hard_link(&database, directory.path().join("hard-link.sqlite"))
            .expect("hard link fixture");
        let third = Host::with_driver("storage-hard-link", "application", driver.clone())?;
        assert_eq!(
            open(&third, &database)
                .await
                .err()
                .expect("hard link refusal")
                .reason,
            "task_storage_unsupported"
        );
        third.close().await
    })
}

#[test]
fn configuration_schema_version_and_corruption_fail_explicitly() -> Result<()> {
    for database in ["", ":memory:", "file:memory?mode=memory"] {
        assert!(
            swarmie_task_storage_sqlite::facet(&Configuration {
                database: database.into(),
                outbox: Default::default(),
            })
            .is_err()
        );
    }
    assert_eq!(
        Configuration {
            database: "".into(),
            outbox: Default::default(),
        }
        .validate(),
        Err(ConfigurationError::EmptyDatabase)
    );
    assert!(
        serde_json::from_value::<Configuration>(
            serde_json::json!({"database":"tasks.sqlite", "unknown":1})
        )
        .is_err()
    );
    let directory = tempfile::tempdir().expect("temporary database directory");
    let database = directory.path().join("tasks.sqlite");
    let unrelated = directory.path().join("unrelated.sqlite");
    rusqlite::Connection::open(&unrelated)
        .expect("unrelated database")
        .execute_batch("CREATE TABLE unrelated(value TEXT);")
        .expect("unrelated schema");
    let driver = core::ManualDriver::new();
    let host = Host::with_driver("storage-initialize", "application", driver.clone())?;
    driver.run_until(async {
        let storage = open(&host, &database).await?;
        swarmie_task_contracts::conformance::run(&storage).await?;
        let persisted = storage
            .get(TaskLookup {
                id: "parent".into(),
            })
            .await?
            .expect("terminal receipt");
        let raw = rusqlite::Connection::open(&database).expect("corruption fixture connection");
        raw.execute(
            "UPDATE task_records SET record = 'not-json' WHERE id = 'parent'",
            [],
        )
        .expect("corrupt fixture");
        assert_eq!(
            storage
                .get(TaskLookup {
                    id: "parent".into()
                })
                .await
                .unwrap_err()
                .reason,
            "task_storage_corrupt"
        );
        let encoded = serde_json::to_string(&persisted).expect("record encoding");
        let duplicate = format!("{{\"phase\":\"duplicate\",{}", &encoded[1..]);
        raw.execute(
            "UPDATE task_records SET record = ?1 WHERE id = 'parent'",
            [duplicate],
        )
        .expect("duplicate field fixture");
        assert_eq!(
            storage
                .get(TaskLookup {
                    id: "parent".into()
                })
                .await
                .unwrap_err()
                .reason,
            "task_storage_corrupt"
        );
        host.close().await?;
        raw.pragma_update(None, "user_version", 999)
            .expect("future schema fixture");
        drop(raw);
        let future = Host::with_driver("storage-future", "application", driver.clone())?;
        assert_eq!(
            open(&future, &database)
                .await
                .err()
                .expect("future schema refusal")
                .reason,
            "task_storage_unsupported"
        );
        future.close().await?;
        let foreign = Host::with_driver("storage-unrelated", "application", driver.clone())?;
        assert_eq!(
            open(&foreign, &unrelated)
                .await
                .err()
                .expect("foreign schema refusal")
                .reason,
            "task_storage_unsupported"
        );
        foreign.close().await
    })
}

#[test]
fn sqlite_failure_after_a_write_rolls_back_the_entire_commit() -> Result<()> {
    use swarmie_task_contracts::{TaskQuery, TaskRecord, TaskSpec, TaskTransaction, TaskWrite};
    let directory = tempfile::tempdir().expect("temporary database directory");
    let database = directory.path().join("tasks.sqlite");
    let driver = core::ManualDriver::new();
    let host = Host::with_driver("storage-failure", "application", driver.clone())?;
    driver.run_until(async {
        let storage = open(&host, &database).await?;
        storage.claim_owner(StorageOwner { id: "failure-owner".into() }).await?;
        let before = storage.scan(TaskQuery::default()).await?;
        let raw = rusqlite::Connection::open(&database).expect("failure injection connection");
        raw.execute_batch("CREATE TRIGGER reject_fixture BEFORE INSERT ON task_records WHEN NEW.id = 'z-rejected' BEGIN SELECT RAISE(FAIL, 'injected write failure'); END;").expect("failure injection trigger");
        let writes = ["a-first", "z-rejected"].into_iter().map(|id| TaskWrite {
            expected_revision: None,
            record: TaskRecord::new(TaskSpec { id: id.into(), kind: "fixture.failure".into(), definition_version: 1, input: serde_json::json!(null), checkpoint: serde_json::json!({"phase":"prepare"}), phase: "prepare".into() }, None),
        }).collect();
        assert_eq!(storage.commit(TaskTransaction { owner: "failure-owner".into(), writes }).await.unwrap_err().reason, "task_storage_io");
        assert_eq!(storage.scan(TaskQuery::default()).await?, before);
        host.close().await
    })
}

#[test]
fn canonical_numeric_records_remain_readable_and_portably_immutable() -> Result<()> {
    use swarmie_task_contracts::{
        CommitResult, TaskQuery, TaskRecord, TaskSpec, TaskTransaction, TaskWrite,
    };
    let directory = tempfile::tempdir().expect("temporary database directory");
    let database = directory.path().join("numeric.sqlite");
    let driver = core::ManualDriver::new();
    let host = Host::with_driver("storage-numeric", "application", driver.clone())?;
    driver.run_until(async {
        let storage = open(&host, &database).await?;
        storage
            .claim_owner(StorageOwner {
                id: "numeric-owner".into(),
            })
            .await?;
        let mut original = TaskRecord::new(
            TaskSpec {
                id: "numeric".into(),
                kind: "fixture.numeric".into(),
                definition_version: 1,
                input: serde_json::json!({"values": vec![1.0; 350_000], "nested":{"number":2.0}}),
                checkpoint: serde_json::Value::Null,
                phase: "ready".into(),
            },
            None,
        );
        assert!(serde_json::to_vec(&original).unwrap().len() > 1_048_576);
        assert!(
            swarmie_task_contracts::task_wire_bytes(&original)?
                < swarmie_task_contracts::MAX_TASK_RECORD_BYTES
        );
        let receipt = storage
            .commit(TaskTransaction {
                owner: "numeric-owner".into(),
                writes: vec![TaskWrite {
                    expected_revision: None,
                    record: original.clone(),
                }],
            })
            .await?;
        let CommitResult::Committed { records, .. } = receipt else {
            panic!("expected creation");
        };
        let committed = records[0].clone();
        assert_eq!(committed.input["nested"]["number"], serde_json::json!(2));
        assert_eq!(
            storage
                .get(TaskLookup {
                    id: "numeric".into()
                })
                .await?,
            Some(committed.clone())
        );
        assert_eq!(
            storage.scan(TaskQuery::default()).await?.records,
            vec![committed]
        );
        // Reuse the original float representation, not the normalized receipt. It denotes
        // the same immutable portable input and must be accepted by the next CAS.
        original.revision = 2;
        original.checkpoint = serde_json::json!({"number":3.0});
        let receipt = storage
            .commit(TaskTransaction {
                owner: "numeric-owner".into(),
                writes: vec![TaskWrite {
                    expected_revision: Some(1),
                    record: original,
                }],
            })
            .await?;
        let CommitResult::Committed { records, .. } = receipt else {
            panic!("expected replacement");
        };
        let updated = records[0].clone();
        assert_eq!(updated.checkpoint["number"], serde_json::json!(3));
        assert_eq!(
            storage
                .get(TaskLookup {
                    id: "numeric".into()
                })
                .await?,
            Some(updated.clone())
        );
        let raw = rusqlite::Connection::open(&database).unwrap();
        let encoded: String = raw
            .query_row(
                "SELECT record FROM task_records WHERE id='numeric'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            encoded.len(),
            swarmie_task_contracts::task_wire_bytes(&updated)?
        );
        host.close().await?;
        let next = Host::with_driver("storage-numeric-reopen", "application", driver.clone())?;
        let reopened = open(&next, &database).await?;
        assert_eq!(
            reopened
                .get(TaskLookup {
                    id: "numeric".into()
                })
                .await?,
            Some(updated)
        );
        next.close().await
    })
}
