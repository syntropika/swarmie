//! Reusable backend conformance through the public TaskStorage Contract.
//!
//! Run against a fresh provider. The suite has no concrete backend dependencies and leaves
//! committed records available for a backend-specific restart or process-crash fixture.

use tembra::prelude::*;

use crate::{
    BlockingReason, CommitResult, StorageOwner, TaskLookup, TaskOutcome, TaskQuery, TaskRecord,
    TaskSpec, TaskStatus, TaskStorageRef, TaskTransaction, TaskWrite, WaitPolicy,
};

const OWNER: &str = "conformance-owner";

fn task(id: &str) -> TaskRecord {
    TaskRecord::new(
        TaskSpec {
            id: id.into(),
            kind: "conformance.task".into(),
            definition_version: 1,
            input: serde_json::json!({"nested": [null, true, 1.0, {"text": "value"}]}),
            checkpoint: serde_json::json!({"phase":"prepare","operation_id":"stable-operation"}),
            phase: "prepare".into(),
        },
        None,
    )
}

fn create(record: TaskRecord) -> TaskWrite {
    TaskWrite {
        expected_revision: None,
        record,
    }
}

fn replace(mut record: TaskRecord) -> TaskWrite {
    let revision = record.revision;
    record.revision += 1;
    TaskWrite {
        expected_revision: Some(revision),
        record,
    }
}

async fn commit(
    storage: &TaskStorageRef,
    writes: Vec<TaskWrite>,
) -> Result<(u64, Vec<TaskRecord>)> {
    match storage
        .commit(TaskTransaction {
            owner: OWNER.into(),
            writes,
        })
        .await?
    {
        CommitResult::Committed { sequence, records } => Ok((sequence, records)),
        result => panic!("expected committed transaction, received {result:?}"),
    }
}

async fn read(storage: &TaskStorageRef, id: &str) -> Result<TaskRecord> {
    Ok(storage
        .get(TaskLookup { id: id.into() })
        .await?
        .expect("committed record"))
}

/// Exercises atomicity, revision checks, ownership, complete replacements, status persistence,
/// bounded consistent pagination and explicit owner admission using only public References.
///
/// # Errors
/// Propagates unexpected provider errors. A failed guarantee panics with the invariant violated.
pub async fn run(storage: &TaskStorageRef) -> Result<()> {
    assert!(
        storage
            .get(TaskLookup {
                id: "absent".into()
            })
            .await?
            .is_none()
    );
    assert!(
        storage
            .commit(TaskTransaction {
                owner: OWNER.into(),
                writes: vec![create(task("before-claim"))]
            })
            .await
            .is_err()
    );
    storage
        .claim_owner(StorageOwner { id: OWNER.into() })
        .await?;
    storage
        .claim_owner(StorageOwner { id: OWNER.into() })
        .await?;
    assert!(
        storage
            .claim_owner(StorageOwner {
                id: "competitor".into()
            })
            .await
            .is_err()
    );
    assert!(
        storage
            .release_owner(StorageOwner {
                id: "competitor".into()
            })
            .await
            .is_err()
    );
    assert!(
        storage
            .commit(TaskTransaction {
                owner: "competitor".into(),
                writes: vec![create(task("wrong-owner"))]
            })
            .await
            .is_err()
    );

    let (sequence, _) = commit(
        storage,
        vec![create(task("parent")), create(task("independent"))],
    )
    .await?;
    assert!(sequence > 0);
    let parent = read(storage, "parent").await?;
    assert_eq!(
        tembra::contracts::canonical_json(&serde_json::to_value(&parent).expect("record JSON"))
            .expect("portable record"),
        tembra::contracts::canonical_json(
            &serde_json::to_value(task("parent")).expect("record JSON")
        )
        .expect("portable record"),
    );
    let mut detached = parent.clone();
    detached.checkpoint["phase"] = serde_json::json!("mutated");
    assert_eq!(read(storage, "parent").await?, parent);

    let conflict = storage
        .commit(TaskTransaction {
            owner: OWNER.into(),
            writes: vec![create(task("parent")), create(task("uncommitted"))],
        })
        .await?;
    assert!(matches!(
        conflict,
        CommitResult::Conflict {
            actual_revision: Some(1),
            ..
        }
    ));
    assert!(
        storage
            .get(TaskLookup {
                id: "uncommitted".into()
            })
            .await?
            .is_none()
    );
    let mut missing = task("missing-update");
    missing.revision = 2;
    assert!(matches!(
        storage
            .commit(TaskTransaction {
                owner: OWNER.into(),
                writes: vec![TaskWrite {
                    expected_revision: Some(1),
                    record: missing
                }]
            })
            .await?,
        CommitResult::Conflict {
            actual_revision: None,
            ..
        }
    ));
    assert!(
        storage
            .commit(TaskTransaction {
                owner: OWNER.into(),
                writes: vec![create(task("duplicate")), create(task("duplicate"))]
            })
            .await
            .is_err()
    );

    let mut unsafe_input = task("unsafe-input");
    unsafe_input.input = serde_json::json!({"nested":[crate::MAX_PORTABLE_INTEGER + 1]});
    assert!(
        storage
            .commit(TaskTransaction {
                owner: OWNER.into(),
                writes: vec![create(unsafe_input)]
            })
            .await
            .is_err()
    );
    assert!(
        storage
            .get(TaskLookup {
                id: "unsafe-input".into()
            })
            .await?
            .is_none()
    );
    let mut changed_kind = read(storage, "independent").await?;
    changed_kind.kind = "changed.task".into();
    assert!(
        storage
            .commit(TaskTransaction {
                owner: OWNER.into(),
                writes: vec![replace(changed_kind)]
            })
            .await
            .is_err()
    );
    let mut changed_input = read(storage, "independent").await?;
    changed_input.input = serde_json::json!({"changed":true});
    assert!(
        storage
            .commit(TaskTransaction {
                owner: OWNER.into(),
                writes: vec![replace(changed_input)]
            })
            .await
            .is_err()
    );
    let too_many = (0..257)
        .map(|index| create(task(&format!("bounded-{index}"))))
        .collect();
    assert!(
        storage
            .commit(TaskTransaction {
                owner: OWNER.into(),
                writes: too_many
            })
            .await
            .is_err()
    );
    let mut blocked = read(storage, "independent").await?;
    blocked.status = TaskStatus::Blocked;
    blocked.blocking_reason = Some(BlockingReason::MissingDefinition);
    let (_, records) = commit(storage, vec![replace(blocked)]).await?;
    assert_eq!(read(storage, "independent").await?, records[0]);
    let mut parked = records[0].clone();
    parked.status = TaskStatus::Parked;
    parked.restarts = 5;
    parked.blocking_reason = Some(BlockingReason::RestartLimit);
    let (_, records) = commit(storage, vec![replace(parked)]).await?;
    assert_eq!(read(storage, "independent").await?, records[0]);

    let mut child_a = task("child-a");
    child_a.parent = Some("parent".into());
    let mut child_b = task("child-b");
    child_b.parent = Some("parent".into());
    let mut waiting = parent.clone();
    waiting.status = TaskStatus::Waiting;
    waiting.wait_for = vec![child_a.id.clone(), child_b.id.clone()];
    waiting.wait_policy = WaitPolicy::AllSettled;
    waiting.checkpoint = serde_json::json!({"phase":"join","children":["child-a","child-b"]});
    waiting.phase = "join".into();
    let (_, records) = commit(
        storage,
        vec![
            replace(waiting),
            create(child_a.clone()),
            create(child_b.clone()),
        ],
    )
    .await?;
    let parent = records[0].clone();
    assert_eq!(read(storage, "parent").await?, parent);
    assert_eq!(
        read(storage, "child-a").await?.parent.as_deref(),
        Some("parent")
    );

    let mut invalid = child_a.clone();
    invalid.parent = Some("independent".into());
    let before = storage.scan(TaskQuery::default()).await?;
    assert!(
        storage
            .commit(TaskTransaction {
                owner: OWNER.into(),
                writes: vec![replace(invalid), create(task("rollback"))]
            })
            .await
            .is_err()
    );
    assert_eq!(storage.scan(TaskQuery::default()).await?, before);
    let stale = storage
        .commit(TaskTransaction {
            owner: OWNER.into(),
            writes: vec![
                TaskWrite {
                    expected_revision: Some(1),
                    record: parent.clone(),
                },
                create(task("stale-rollback")),
            ],
        })
        .await?;
    assert!(matches!(
        stale,
        CommitResult::Conflict {
            actual_revision: Some(2),
            ..
        }
    ));
    assert!(
        storage
            .get(TaskLookup {
                id: "stale-rollback".into()
            })
            .await?
            .is_none()
    );
    let mut foreign_wait = parent.clone();
    foreign_wait.wait_for = vec!["independent".into()];
    assert!(
        storage
            .commit(TaskTransaction {
                owner: OWNER.into(),
                writes: vec![replace(foreign_wait)]
            })
            .await
            .is_err()
    );
    let mut missing_parent = task("missing-parent");
    missing_parent.parent = Some("unknown-parent".into());
    assert!(
        storage
            .commit(TaskTransaction {
                owner: OWNER.into(),
                writes: vec![create(missing_parent)]
            })
            .await
            .is_err()
    );
    let mut cycle_a = task("cycle-a");
    cycle_a.parent = Some("cycle-b".into());
    let mut cycle_b = task("cycle-b");
    cycle_b.parent = Some("cycle-a".into());
    assert!(
        storage
            .commit(TaskTransaction {
                owner: OWNER.into(),
                writes: vec![create(cycle_a), create(cycle_b)]
            })
            .await
            .is_err()
    );

    let first = storage
        .scan(TaskQuery {
            limit: 1,
            ..Default::default()
        })
        .await?;
    assert_eq!(first.records.len(), 1);
    let cursor = first.next.clone().expect("more records");
    let second = storage
        .scan(TaskQuery {
            limit: 1,
            cursor: Some(cursor.clone()),
            ..Default::default()
        })
        .await?;
    assert!(second.records[0].id > first.records[0].id);
    assert!(
        storage
            .scan(TaskQuery {
                limit: 0,
                ..Default::default()
            })
            .await
            .is_err()
    );
    assert!(
        storage
            .scan(TaskQuery {
                limit: 257,
                ..Default::default()
            })
            .await
            .is_err()
    );
    assert!(
        storage
            .scan(TaskQuery {
                cursor: Some(cursor.clone()),
                parent: Some("parent".into()),
                ..Default::default()
            })
            .await
            .is_err()
    );
    let mut foreign_cursor = cursor.clone();
    foreign_cursor.store_id = "another-provider".into();
    assert!(
        storage
            .scan(TaskQuery {
                cursor: Some(foreign_cursor),
                ..Default::default()
            })
            .await
            .is_err()
    );
    let children = storage
        .scan(TaskQuery {
            parent: Some("parent".into()),
            ..Default::default()
        })
        .await?;
    assert_eq!(
        children
            .records
            .iter()
            .map(|record| record.id.as_str())
            .collect::<Vec<_>>(),
        ["child-a", "child-b"]
    );

    let mut running = child_a;
    running.status = TaskStatus::Running;
    running.attempts = 1;
    let (_, records) = commit(storage, vec![replace(running)]).await?;
    let mut running = records[0].clone();
    assert!(
        storage
            .scan(TaskQuery {
                cursor: Some(cursor),
                ..Default::default()
            })
            .await
            .is_err()
    );
    running.status = TaskStatus::Pending;
    running.restarts = 1;
    let (_, records) = commit(storage, vec![replace(running)]).await?;
    let mut sleeping = records[0].clone();
    sleeping.status = TaskStatus::Sleeping;
    sleeping.wake_at_ms = Some(123456);
    let (_, records) = commit(storage, vec![replace(sleeping)]).await?;
    let mut cancellation = records[0].clone();
    cancellation.cancellation_requested = true;
    let (_, records) = commit(storage, vec![replace(cancellation)]).await?;
    let mut cancellation = records[0].clone();
    cancellation.cancellation_requested = false;
    assert!(
        storage
            .commit(TaskTransaction {
                owner: OWNER.into(),
                writes: vec![replace(cancellation)]
            })
            .await
            .is_err()
    );

    let mut completing = parent.clone();
    completing.status = TaskStatus::Completing;
    completing.wait_for.clear();
    completing.outcome = Some(TaskOutcome::Completed {
        value: serde_json::json!({"joined":true}),
    });
    let (_, records) = commit(storage, vec![replace(completing)]).await?;
    let completing = records[0].clone();
    let mut late = task("late-child");
    late.parent = Some("parent".into());
    assert!(
        storage
            .commit(TaskTransaction {
                owner: OWNER.into(),
                writes: vec![create(late)]
            })
            .await
            .is_err()
    );
    let mut terminal = completing.clone();
    terminal.status = TaskStatus::Terminal;
    assert!(
        storage
            .commit(TaskTransaction {
                owner: OWNER.into(),
                writes: vec![replace(terminal.clone())]
            })
            .await
            .is_err()
    );
    let mut child_a = read(storage, "child-a").await?;
    child_a.status = TaskStatus::Terminal;
    child_a.wake_at_ms = None;
    child_a.outcome = Some(TaskOutcome::Aborted {
        reason: "requested".into(),
    });
    let mut child_b = read(storage, "child-b").await?;
    child_b.status = TaskStatus::Terminal;
    child_b.outcome = Some(TaskOutcome::Completed {
        value: serde_json::json!(null),
    });
    let (_, records) = commit(
        storage,
        vec![replace(terminal), replace(child_a), replace(child_b)],
    )
    .await?;
    assert!(records.iter().all(TaskRecord::is_terminal));
    assert!(
        storage
            .commit(TaskTransaction {
                owner: OWNER.into(),
                writes: vec![replace(records[0].clone())]
            })
            .await
            .is_err()
    );
    let terminal_page = storage
        .scan(TaskQuery {
            statuses: vec![TaskStatus::Terminal],
            ..Default::default()
        })
        .await?;
    assert_eq!(terminal_page.records.len(), 3);
    storage
        .release_owner(StorageOwner { id: OWNER.into() })
        .await?;
    storage
        .claim_owner(StorageOwner {
            id: "successor-owner".into(),
        })
        .await?;
    assert_eq!(read(storage, "parent").await?.checkpoint["phase"], "join");
    storage
        .release_owner(StorageOwner {
            id: "successor-owner".into(),
        })
        .await
}
