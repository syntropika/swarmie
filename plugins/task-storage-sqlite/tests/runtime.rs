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
                database: database.into()
            })
            .is_err()
        );
    }
    assert_eq!(
        Configuration {
            database: "".into()
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
