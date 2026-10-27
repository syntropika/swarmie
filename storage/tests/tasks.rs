#![cfg(feature = "sqlite")]

use futures::executor::block_on;
use std::num::{NonZeroU32, NonZeroU64};
use swarmie_durable_tasks::*;
use swarmie_plugin_manager::{PluginStorage, StorageError as InventoryError};
use swarmie_storage::OwnerError;
use swarmie_storage::sqlite::{SqliteOwner, SqliteTaskStorage};
fn spec() -> TaskSpec {
    TaskSpec {
        id: TaskId::new(),
        definition: TaskDefinition::new("fixture.documents", NonZeroU32::new(1).unwrap()).unwrap(),
        input: 100.into(),
    }
}
fn create(spec: TaskSpec) -> TaskChange {
    TaskChange::Create {
        spec,
        principal: "alice".into(),
    }
}
fn claim(spec: &TaskSpec, attempt: AttemptId) -> TaskChange {
    TaskChange::Acquire {
        id: spec.id,
        revision: 0,
        executor: ExecutorId::new(),
        attempt,
        lease_ms: NonZeroU64::new(30_000).unwrap(),
    }
}
fn applied(outcome: CommitOutcome) {
    assert!(
        matches!(outcome, CommitOutcome::Applied { .. }),
        "{outcome:?}"
    );
}
#[test]
fn competing_owners_cannot_acquire_the_same_unexpired_assignment() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("tasks.sqlite");
    let first = SqliteTaskStorage::open(&file).unwrap();
    let second = SqliteTaskStorage::open(&file).unwrap();
    let spec = spec();
    let attempt = AttemptId::new();
    block_on(async {
        applied(
            first
                .commit(vec![create(spec.clone()), claim(&spec, attempt)])
                .await
                .unwrap(),
        );
        assert!(matches!(
            second
                .commit(vec![claim(&spec, AttemptId::new())])
                .await
                .unwrap(),
            CommitOutcome::Rejected(Rejection::Assigned)
        ));
        applied(
            first
                .commit(vec![TaskChange::Checkpoint {
                    id: spec.id,
                    attempt,
                    revision: 0,
                    checkpoint: 40.into(),
                }])
                .await
                .unwrap(),
        );
        first.close().await.unwrap();
        second.close().await.unwrap();
        let reopened = SqliteTaskStorage::open(&file).unwrap();
        let record = reopened.get(spec.id).await.unwrap().unwrap();
        assert_eq!(record.checkpoint, Some(40.into()));
        assert_eq!(record.revision, 1);
        assert_eq!(record.spec.input, serde_json::Value::from(100));
        reopened.close().await.unwrap();
        assert!(reopened.get(spec.id).await.is_err());
    });
}
#[test]
fn a_later_sql_failure_rolls_back_batch_and_sequence() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("tasks.sqlite");
    let storage = SqliteTaskStorage::open(&file).unwrap();
    let connection = rusqlite::Connection::open(&file).unwrap();
    connection.execute_batch("CREATE TRIGGER reject_task BEFORE INSERT ON swarmie_tasks WHEN (SELECT count(*) FROM swarmie_tasks)>0 BEGIN SELECT RAISE(ABORT,'injected refusal'); END;").unwrap();
    block_on(async {
        assert!(
            storage
                .commit(vec![create(spec()), create(spec())])
                .await
                .is_err()
        );
        assert!(
            storage
                .list(None, TaskPageRequest::default())
                .await
                .unwrap()
                .is_empty()
        );
        connection
            .execute_batch("DROP TRIGGER reject_task;")
            .unwrap();
        let s = spec();
        applied(storage.commit(vec![create(s.clone())]).await.unwrap());
        assert_eq!(storage.get(s.id).await.unwrap().unwrap().sequence, 1);
        storage.close().await.unwrap();
    });
}
#[test]
fn targeted_reads_and_metadata_pages_do_not_decode_unrelated_payloads() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("tasks.sqlite");
    let storage = SqliteTaskStorage::open(&file).unwrap();
    let bad = spec();
    let good = spec();
    block_on(storage.commit(vec![create(bad.clone()), create(good.clone())])).unwrap();
    let connection = rusqlite::Connection::open(&file).unwrap();
    connection
        .execute(
            "UPDATE swarmie_tasks SET data='broken json' WHERE id=?1",
            [bad.id.to_string()],
        )
        .unwrap();
    block_on(async {
        assert!(matches!(
            storage.get(bad.id).await,
            Err(StorageError::Invalid(_))
        ));
        assert_eq!(storage.get(good.id).await.unwrap().unwrap().spec, good);
        let page = storage
            .list(
                None,
                TaskPageRequest {
                    after: 0,
                    limit: NonZeroU32::new(1).unwrap(),
                },
            )
            .await
            .unwrap();
        assert_eq!(page.len(), 1);
        assert_eq!(page[0].id, bad.id);
        assert_eq!(
            storage
                .candidates(page[0].sequence, 1)
                .await
                .unwrap()
                .candidates[0]
                .id,
            good.id
        );
        storage.close().await.unwrap();
    });
}
#[test]
fn mismatched_stored_identity_is_not_missing_work() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("tasks.sqlite");
    let storage = SqliteTaskStorage::open(&file).unwrap();
    block_on(storage.commit(vec![create(spec())])).unwrap();
    let id = TaskId::new();
    let connection = rusqlite::Connection::open(&file).unwrap();
    connection
        .execute("UPDATE swarmie_tasks SET id=?1", [id.to_string()])
        .unwrap();
    assert!(matches!(
        block_on(storage.get(id)),
        Err(StorageError::Invalid(_))
    ));
    block_on(storage.close()).unwrap();
}
#[test]
fn renewal_and_release_do_not_rewrite_payloads() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("tasks.sqlite");
    let storage = SqliteTaskStorage::open(&file).unwrap();
    let s = spec();
    let attempt = AttemptId::new();
    block_on(storage.commit(vec![
        create(s.clone()),
        claim(&s, attempt),
        TaskChange::Checkpoint {
            id: s.id,
            attempt,
            revision: 0,
            checkpoint: 40.into(),
        },
    ]))
    .unwrap();
    let connection = rusqlite::Connection::open(&file).unwrap();
    connection.execute_batch("CREATE TRIGGER preserve_input BEFORE UPDATE OF data ON swarmie_tasks BEGIN SELECT RAISE(ABORT,'input rewrite'); END; CREATE TRIGGER preserve_progress BEFORE UPDATE OF progress ON swarmie_tasks BEGIN SELECT RAISE(ABORT,'checkpoint rewrite'); END;").unwrap();
    block_on(async {
        applied(
            storage
                .commit(vec![
                    TaskChange::Renew {
                        id: s.id,
                        attempt,
                        lease_ms: NonZeroU64::new(30_000).unwrap(),
                    },
                    TaskChange::Release { id: s.id, attempt },
                ])
                .await
                .unwrap(),
        );
        let r = storage.get(s.id).await.unwrap().unwrap();
        assert_eq!(r.checkpoint, Some(40.into()));
        assert_eq!(r.revision, 1);
        assert!(r.assignment.is_none());
        storage.close().await.unwrap();
    });
}
#[test]
fn shared_owner_freezes_all_capabilities_and_closes_only_after_last_lease() {
    let dir = tempfile::tempdir().unwrap();
    let owner = SqliteOwner::new(dir.path().join("tasks.sqlite"));
    let inventory = owner.inventory().unwrap();
    let tasks = owner.tasks().unwrap();
    assert!(inventory.owner().same(&tasks.owner()));
    block_on(inventory.close()).unwrap();
    assert!(owner.authority().check().is_ok());
    block_on(tasks.commit(vec![create(spec())])).unwrap();
    owner.authority().require_recovery();
    assert!(matches!(
        block_on(tasks.commit(vec![create(spec())])),
        Err(StorageError::Owner(OwnerError::RecoveryRequired))
    ));
    block_on(tasks.close()).unwrap();
    assert_eq!(owner.authority().check(), Err(OwnerError::Closed));
    let owner = SqliteOwner::new(dir.path().join("other.sqlite"));
    let inventory = owner.inventory().unwrap();
    let tasks = owner.tasks().unwrap();
    tasks.owner().require_recovery();
    assert!(matches!(
        block_on(inventory.commit(0, vec![])),
        Err(InventoryError::Owner(OwnerError::RecoveryRequired))
    ));
    block_on(tasks.close()).unwrap();
    assert!(
        block_on(inventory.inventory()).is_ok(),
        "read evidence remains available while frozen"
    );
    block_on(inventory.close()).unwrap();
}
#[test]
fn checkpoint_survives_abrupt_process_exit() {
    const CHILD_PATH: &str = "SWARMIE_TASK_CRASH_TEST_PATH";
    if let Some(file) = std::env::var_os(CHILD_PATH) {
        let storage = SqliteTaskStorage::open(file).unwrap();
        let s = spec();
        let attempt = AttemptId::new();
        applied(
            block_on(storage.commit(vec![
                create(s.clone()),
                claim(&s, attempt),
                TaskChange::Checkpoint {
                    id: s.id,
                    attempt,
                    revision: 0,
                    checkpoint: 40.into(),
                },
            ]))
            .unwrap(),
        );
        std::process::exit(0);
    }
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("crash.sqlite");
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "checkpoint_survives_abrupt_process_exit"])
        .env(CHILD_PATH, &file)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let storage = SqliteTaskStorage::open(file).unwrap();
    block_on(async {
        let id = storage
            .list(None, TaskPageRequest::default())
            .await
            .unwrap()[0]
            .id;
        let record = storage.get(id).await.unwrap().unwrap();
        assert_eq!(record.checkpoint, Some(40.into()));
        assert_eq!(record.revision, 1);
        assert!(record.assignment.is_some());
        storage.close().await.unwrap();
    });
}

#[test]
fn list_caps_requested_size_and_invalid_cursors_are_not_transient() {
    let dir = tempfile::tempdir().unwrap();
    let storage = SqliteTaskStorage::open(dir.path().join("tasks.sqlite")).unwrap();
    block_on(async {
        applied(
            storage
                .commit((0..1001).map(|_| create(spec())).collect())
                .await
                .unwrap(),
        );
        let page = storage
            .list(
                Some("alice".into()),
                TaskPageRequest {
                    after: 0,
                    limit: NonZeroU32::new(u32::MAX).unwrap(),
                },
            )
            .await
            .unwrap();
        assert_eq!(page.len(), TaskPageRequest::MAX_LIMIT.get() as usize);
        assert_eq!(
            storage
                .list(
                    Some("alice".into()),
                    TaskPageRequest {
                        after: page.last().unwrap().sequence,
                        limit: TaskPageRequest::MAX_LIMIT,
                    }
                )
                .await
                .unwrap()
                .len(),
            1
        );
        assert!(matches!(
            storage
                .list(
                    None,
                    TaskPageRequest {
                        after: u64::MAX,
                        limit: NonZeroU32::new(1).unwrap(),
                    }
                )
                .await,
            Err(StorageError::Invalid(_))
        ));
        storage.close().await.unwrap();
    });
}
