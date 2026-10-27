use futures::executor::block_on;
use serde_json::{Value, json};
use std::num::{NonZeroU32, NonZeroU64};
use swarmie_durable_tasks::*;
use swarmie_storage_sqlite::SqliteTaskStorage;

fn create(storage: &SqliteTaskStorage) -> (TaskId, AttemptId, SignalKey) {
    let id = TaskId::new();
    let attempt = AttemptId::new();
    let key = SignalKey::new();
    applied(
        storage,
        vec![
            TaskChange::Create {
                spec: TaskSpec {
                    id,
                    definition: TaskDefinition::new(
                        "fixture.signal_task",
                        NonZeroU32::new(1).unwrap(),
                    )
                    .unwrap(),
                    input: json!(0),
                },
                principal: "alice".into(),
            },
            acquire(id, attempt, 0),
        ],
    );
    (id, attempt, key)
}
fn acquire(id: TaskId, attempt: AttemptId, revision: u64) -> TaskChange {
    TaskChange::Acquire {
        id,
        attempt,
        revision,
        executor: ExecutorId::new(),
        lease_ms: NonZeroU64::new(30000).unwrap(),
    }
}
fn signal(id: TaskId, key: SignalKey, value: Value) -> TaskChange {
    TaskChange::Signal { id, key, value }
}
fn wait(id: TaskId, attempt: AttemptId, key: SignalKey, revision: u64) -> TaskChange {
    TaskChange::Suspend {
        id,
        attempt,
        revision,
        checkpoint: json!("saved"),
        condition: WaitFor::Signal(key),
    }
}
fn applied(storage: &SqliteTaskStorage, changes: Vec<TaskChange>) {
    let outcome = block_on(storage.commit(changes)).unwrap();
    assert!(
        matches!(outcome, CommitOutcome::Applied { .. }),
        "{outcome:?}"
    );
}
fn receipt(connection: &rusqlite::Connection, id: TaskId, key: SignalKey) -> bool {
    connection
        .query_row(
            "SELECT consumed FROM swarmie_task_signals WHERE task=?1 AND key=?2",
            rusqlite::params![id.to_string(), key.to_string()],
            |row| row.get(0),
        )
        .unwrap()
}

#[test]
fn signal_sql_write_rolls_back_with_checkpoint_write_failure() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rollback.sqlite");
    let s = SqliteTaskStorage::open(&path).unwrap();
    let (id, a, key) = create(&s);
    applied(
        &s,
        vec![wait(id, a, key, 0), TaskChange::Release { id, attempt: a }],
    );
    let c = rusqlite::Connection::open(&path).unwrap();
    c.execute_batch("CREATE TRIGGER reject_progress BEFORE UPDATE OF progress ON swarmie_tasks BEGIN SELECT RAISE(ABORT,'injected checkpoint refusal'); END;").unwrap();
    assert!(block_on(s.commit(vec![signal(id, key, json!(1))])).is_err());
    assert_eq!(
        c.query_row("SELECT count(*) FROM swarmie_task_signals", [], |r| r
            .get::<_, u64>(0))
            .unwrap(),
        0
    );
    let r = block_on(s.get(id)).unwrap().unwrap();
    assert_eq!(r.status, TaskStatus::Waiting(WaitState::Signal(key)));
    assert_eq!(r.checkpoint, Some(json!("saved")));
    assert_eq!(r.revision, 1);
    c.execute_batch("DROP TRIGGER reject_progress;").unwrap();
    applied(&s, vec![signal(id, key, json!(1))]);
    assert!(receipt(&c, id, key));
    block_on(s.close()).unwrap();
}

#[test]
fn early_and_duplicate_signals_do_not_rewrite_the_checkpoint() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("targeted.sqlite");
    let s = SqliteTaskStorage::open(&path).unwrap();
    let (id, a, key) = create(&s);
    applied(
        &s,
        vec![TaskChange::Checkpoint {
            id,
            attempt: a,
            revision: 0,
            checkpoint: json!("x".repeat(65536)),
        }],
    );
    let c = rusqlite::Connection::open(path).unwrap();
    c.execute_batch("CREATE TRIGGER preserve_progress BEFORE UPDATE OF progress ON swarmie_tasks BEGIN SELECT RAISE(ABORT,'unexpected checkpoint rewrite'); END;").unwrap();
    applied(&s, vec![signal(id, key, json!(7))]);
    applied(&s, vec![signal(id, key, json!(7))]);
    assert!(!receipt(&c, id, key));
    let raw: String = c
        .query_row(
            "SELECT progress FROM swarmie_tasks WHERE id=?1",
            [id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    let raw: Value = serde_json::from_str(&raw).unwrap();
    assert!(raw.get("signals").is_none());
    let r = block_on(s.get(id)).unwrap().unwrap();
    assert_eq!(r.revision, 1);
    assert_eq!(r.checkpoint, Some(json!("x".repeat(65536))));
    block_on(s.close()).unwrap();
}

#[test]
fn multiple_keys_and_duplicate_evidence_survive_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipts.sqlite");
    let s = SqliteTaskStorage::open(&path).unwrap();
    let (id, a, key) = create(&s);
    let next = SignalKey::new();
    let outcome = block_on(s.commit(vec![
        signal(id, key, json!(1)),
        signal(id, next, json!(2)),
        TaskChange::Release {
            id: TaskId::new(),
            attempt: a,
        },
    ]))
    .unwrap();
    assert!(matches!(
        outcome,
        CommitOutcome::Rejected(Rejection::NotFound)
    ));
    let c = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        c.query_row("SELECT count(*) FROM swarmie_task_signals", [], |r| r
            .get::<_, u64>(0))
            .unwrap(),
        0
    );
    applied(
        &s,
        vec![
            signal(id, key, json!(1)),
            signal(id, next, json!(2)),
            wait(id, a, key, 0),
            TaskChange::Release { id, attempt: a },
        ],
    );
    assert!(receipt(&c, id, key));
    assert!(!receipt(&c, id, next));
    block_on(s.close()).unwrap();
    let s = SqliteTaskStorage::open(&path).unwrap();
    applied(&s, vec![signal(id, key, json!(1))]);
    assert!(matches!(
        block_on(s.commit(vec![signal(id, key, json!(3))])).unwrap(),
        CommitOutcome::Rejected(Rejection::Invalid)
    ));
    let b = AttemptId::new();
    applied(
        &s,
        vec![
            acquire(id, b, 1),
            wait(id, b, next, 1),
            TaskChange::Release { id, attempt: b },
        ],
    );
    let r = block_on(s.get(id)).unwrap().unwrap();
    assert_eq!(r.delivery.unwrap().value, json!(2));
    assert!(receipt(&c, id, key));
    assert!(receipt(&c, id, next));
    let b = AttemptId::new();
    applied(&s, vec![acquire(id, b, 2)]);
    assert!(matches!(
        block_on(s.commit(vec![wait(id, b, key, 2)])).unwrap(),
        CommitOutcome::Rejected(Rejection::Invalid)
    ));
    assert_eq!(block_on(s.get(id)).unwrap().unwrap().revision, 2);
    block_on(s.close()).unwrap();
}

#[test]
fn stale_attempt_cannot_consume_a_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fence.sqlite");
    let s = SqliteTaskStorage::open(&path).unwrap();
    let (id, a, key) = create(&s);
    let b = AttemptId::new();
    applied(
        &s,
        vec![
            signal(id, key, json!(1)),
            TaskChange::Release { id, attempt: a },
            acquire(id, b, 0),
        ],
    );
    assert!(matches!(
        block_on(s.commit(vec![wait(id, a, key, 0)])).unwrap(),
        CommitOutcome::Rejected(Rejection::StaleAttempt)
    ));
    let c = rusqlite::Connection::open(path).unwrap();
    assert!(!receipt(&c, id, key));
    applied(
        &s,
        vec![wait(id, b, key, 0), TaskChange::Release { id, attempt: b }],
    );
    assert!(receipt(&c, id, key));
    block_on(s.close()).unwrap();
}

#[test]
fn frozen_owner_refuses_signal_writes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("frozen.sqlite");
    let s = SqliteTaskStorage::open(&path).unwrap();
    let (id, _, key) = create(&s);
    s.owner().require_recovery();
    assert!(matches!(
        block_on(s.commit(vec![signal(id, key, json!(1))])),
        Err(StorageError::Owner(_))
    ));
    let c = rusqlite::Connection::open(path).unwrap();
    assert_eq!(
        c.query_row("SELECT count(*) FROM swarmie_task_signals", [], |r| r
            .get::<_, u64>(0))
            .unwrap(),
        0
    );
    block_on(s.close()).unwrap();
}
