use futures::executor::block_on;
use serde_json::json;
use std::num::{NonZeroU32, NonZeroU64};
use swarmie_durable_tasks::*;
use swarmie_storage_sqlite::SqliteTaskStorage;

fn task() -> TaskSpec {
    TaskSpec {
        id: TaskId::new(),
        definition: TaskDefinition::new("fixture.wait", NonZeroU32::new(1).unwrap()).unwrap(),
        input: json!(1),
    }
}
fn acquire(id: TaskId, attempt: AttemptId, revision: u64) -> TaskChange {
    TaskChange::Acquire {
        id,
        attempt,
        revision,
        executor: ExecutorId::new(),
        lease_ms: NonZeroU64::new(30_000).unwrap(),
    }
}
fn applied(storage: &SqliteTaskStorage, changes: Vec<TaskChange>) {
    assert!(matches!(
        block_on(storage.commit(changes)).unwrap(),
        CommitOutcome::Applied { .. }
    ));
}
fn create(storage: &SqliteTaskStorage) -> (TaskId, AttemptId) {
    let spec = task();
    let id = spec.id;
    let attempt = AttemptId::new();
    applied(
        storage,
        vec![
            TaskChange::Create {
                spec,
                principal: "alice".into(),
            },
            acquire(id, attempt, 0),
        ],
    );
    (id, attempt)
}
fn suspend(id: TaskId, attempt: AttemptId, condition: WaitFor) -> TaskChange {
    TaskChange::Suspend {
        id,
        attempt,
        revision: 0,
        checkpoint: json!({"saved":true}),
        condition,
    }
}

#[test]
fn checkpoint_wait_and_release_are_atomic_and_survive_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("wait.sqlite");
    let storage = SqliteTaskStorage::open(&path).unwrap();
    let (id, attempt) = create(&storage);
    let before = block_on(storage.get(id)).unwrap().unwrap();
    let invalid = block_on(storage.commit(vec![
        suspend(
            id,
            attempt,
            WaitFor::Delay {
                milliseconds: 60_000,
            },
        ),
        TaskChange::Release { id, attempt },
        TaskChange::Signal {
            id: TaskId::new(),
            key: SignalKey::new(),
            value: json!(7),
        },
    ]))
    .unwrap();
    assert!(matches!(
        invalid,
        CommitOutcome::Rejected(Rejection::NotFound)
    ));
    assert_eq!(block_on(storage.get(id)).unwrap().unwrap(), before);
    applied(
        &storage,
        vec![
            suspend(
                id,
                attempt,
                WaitFor::Delay {
                    milliseconds: 60_000,
                },
            ),
            TaskChange::Release { id, attempt },
        ],
    );
    let waiting = block_on(storage.get(id)).unwrap().unwrap();
    let TaskStatus::Waiting(WaitState::Until(deadline)) = waiting.status else {
        panic!("expected a durable deadline")
    };
    assert_eq!(waiting.checkpoint, Some(json!({"saved":true})));
    assert_eq!(waiting.revision, 1);
    assert!(waiting.assignment.is_none());
    let candidates = block_on(storage.candidates(0, 64)).unwrap();
    assert!(candidates.candidates.is_empty());
    assert_eq!(candidates.next_wake, Some(deadline));
    block_on(storage.close()).unwrap();
    let reopened = SqliteTaskStorage::open(path).unwrap();
    assert_eq!(block_on(reopened.get(id)).unwrap().unwrap(), waiting);
    block_on(reopened.close()).unwrap();
}

#[test]
fn indexed_candidates_merge_ready_and_due_work_and_report_future_deadlines() {
    let dir = tempfile::tempdir().unwrap();
    let storage = SqliteTaskStorage::open(dir.path().join("indexed.sqlite")).unwrap();
    let (future, a) = create(&storage);
    applied(
        &storage,
        vec![
            suspend(
                future,
                a,
                WaitFor::Delay {
                    milliseconds: 3_600_000,
                },
            ),
            TaskChange::Release {
                id: future,
                attempt: a,
            },
        ],
    );
    let (due, a) = create(&storage);
    applied(
        &storage,
        vec![
            suspend(due, a, WaitFor::Delay { milliseconds: 0 }),
            TaskChange::Release {
                id: due,
                attempt: a,
            },
        ],
    );
    let spec = task();
    let ready = spec.id;
    applied(
        &storage,
        vec![TaskChange::Create {
            spec,
            principal: "alice".into(),
        }],
    );
    let candidates = block_on(storage.candidates(0, 64)).unwrap();
    assert_eq!(
        candidates
            .candidates
            .iter()
            .map(|candidate| candidate.id)
            .collect::<Vec<_>>(),
        vec![due, ready]
    );
    let TaskStatus::Waiting(WaitState::Until(deadline)) =
        block_on(storage.get(future)).unwrap().unwrap().status
    else {
        panic!("expected future wait")
    };
    assert_eq!(candidates.next_wake, Some(deadline));
    let next = block_on(storage.candidates(candidates.candidates[0].sequence, 1)).unwrap();
    assert_eq!(next.candidates[0].id, ready);
    assert_eq!(next.next_wake, Some(deadline));
    block_on(storage.close()).unwrap();
}

#[test]
fn waits_survive_abrupt_exit_and_terminal_receipts_survive_reopen() {
    const CHILD_DATABASE: &str = "SWARMIE_WAIT_TEST_CHILD_DATABASE";
    if let Some(path) = std::env::var_os(CHILD_DATABASE) {
        let storage = SqliteTaskStorage::open(path).unwrap();
        for condition in [
            WaitFor::Delay {
                milliseconds: 3_600_000,
            },
            WaitFor::Signal(SignalKey::new()),
        ] {
            let (id, attempt) = create(&storage);
            applied(
                &storage,
                vec![
                    suspend(id, attempt, condition),
                    TaskChange::Release { id, attempt },
                ],
            );
        }
        // Exit without close or destructor cleanup after committed durable waits.
        std::process::exit(0);
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("crash.sqlite");
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "waits_survive_abrupt_exit_and_terminal_receipts_survive_reopen",
        ])
        .env(CHILD_DATABASE, &path)
        .status()
        .unwrap();
    assert!(status.success());
    let storage = SqliteTaskStorage::open(&path).unwrap();
    let records = block_on(storage.list(None, TaskPageRequest::default())).unwrap();
    assert_eq!(records.len(), 2);
    for metadata in records {
        let record = block_on(storage.get(metadata.id)).unwrap().unwrap();
        assert_eq!(record.checkpoint, Some(json!({"saved":true})));
        assert!(record.assignment.is_none());
        if let TaskStatus::Waiting(WaitState::Signal(key)) = record.status {
            let id = record.spec.id;
            applied(
                &storage,
                vec![TaskChange::Signal {
                    id,
                    key,
                    value: json!(7),
                }],
            );
            let attempt = AttemptId::new();
            applied(
                &storage,
                vec![
                    acquire(id, attempt, 1),
                    TaskChange::Complete {
                        id,
                        attempt,
                        revision: 1,
                        result: json!(9),
                    },
                    TaskChange::Release { id, attempt },
                ],
            );
            block_on(storage.close()).unwrap();
            let reopened = SqliteTaskStorage::open(&path).unwrap();
            applied(
                &reopened,
                vec![TaskChange::Signal {
                    id,
                    key,
                    value: json!(7),
                }],
            );
            assert!(matches!(
                block_on(reopened.commit(vec![TaskChange::Signal {
                    id,
                    key,
                    value: json!(8)
                }]))
                .unwrap(),
                CommitOutcome::Rejected(Rejection::Invalid)
            ));
            let final_record = block_on(reopened.get(id)).unwrap().unwrap();
            assert_eq!(final_record.status, TaskStatus::Completed(json!(9)));
            assert_eq!(final_record.delivery.unwrap().value, json!(7));
            block_on(reopened.close()).unwrap();
            return;
        }
    }
    panic!("missing recovered signal wait");
}
