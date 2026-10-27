use std::{
    collections::BTreeMap,
    num::{NonZeroU32, NonZeroU64},
};
use swarmie_durable_tasks::*;
fn spec() -> TaskSpec {
    TaskSpec {
        id: TaskId::new(),
        definition: TaskDefinition::new("fixture.task", NonZeroU32::new(1).unwrap()).unwrap(),
        input: 100.into(),
    }
}
fn acquire(id: TaskId, attempt: AttemptId) -> TaskChange {
    TaskChange::Acquire {
        id,
        revision: 0,
        executor: ExecutorId::new(),
        attempt,
        lease_ms: NonZeroU64::new(10).unwrap(),
    }
}
fn initial() -> (PreparedTasks, TaskId, AttemptId) {
    let s = spec();
    let id = s.id;
    let attempt = AttemptId::new();
    let prepared = apply_changes(
        &BTreeMap::new(),
        &swarmie_durable_tasks::SignalSnapshot::new(),
        10,
        0,
        vec![
            TaskChange::Create {
                spec: s,
                principal: "alice".into(),
            },
            acquire(id, attempt),
        ],
    )
    .unwrap();
    (prepared, id, attempt)
}
#[test]
fn rejection_preserves_records_and_creation_sequence() {
    let (p, id, attempt) = initial();
    let before = p.records.clone();
    let new = spec();
    let result = apply_changes(
        &p.records,
        &swarmie_durable_tasks::SignalSnapshot::new(),
        11,
        p.sequence,
        vec![
            TaskChange::Create {
                spec: new,
                principal: "alice".into(),
            },
            TaskChange::Checkpoint {
                id,
                attempt,
                revision: 7,
                checkpoint: 40.into(),
            },
        ],
    );
    assert!(matches!(result, Err(Rejection::Revision)));
    assert_eq!(p.records, before);
    assert_eq!(p.sequence, 1);
}
#[test]
fn expiry_boundary_refuses_progress_and_renewal() {
    let (p, id, attempt) = initial();
    assert!(matches!(
        apply_changes(
            &p.records,
            &swarmie_durable_tasks::SignalSnapshot::new(),
            20,
            p.sequence,
            vec![TaskChange::Checkpoint {
                id,
                attempt,
                revision: 0,
                checkpoint: 40.into()
            }]
        ),
        Err(Rejection::StaleAttempt)
    ));
    assert!(matches!(
        apply_changes(
            &p.records,
            &swarmie_durable_tasks::SignalSnapshot::new(),
            20,
            p.sequence,
            vec![TaskChange::Renew {
                id,
                attempt,
                lease_ms: NonZeroU64::new(10).unwrap()
            }]
        ),
        Err(Rejection::StaleAttempt)
    ));
}
#[test]
fn stale_attempt_cannot_release_or_commit_over_successor() {
    let (p, id, old) = initial();
    let successor = AttemptId::new();
    let next = apply_changes(
        &p.records,
        &swarmie_durable_tasks::SignalSnapshot::new(),
        20,
        p.sequence,
        vec![acquire(id, successor)],
    )
    .unwrap();
    for change in [
        TaskChange::Release { id, attempt: old },
        TaskChange::Checkpoint {
            id,
            attempt: old,
            revision: 0,
            checkpoint: 40.into(),
        },
    ] {
        assert!(matches!(
            apply_changes(
                &next.records,
                &swarmie_durable_tasks::SignalSnapshot::new(),
                21,
                next.sequence,
                vec![change]
            ),
            Err(Rejection::StaleAttempt)
        ));
    }
    assert_eq!(
        next.records[&id].assignment.as_ref().unwrap().attempt,
        successor
    );
}
#[test]
fn assignment_changes_preserve_checkpoint_and_progress_revision() {
    let (p, id, attempt) = initial();
    let next = apply_changes(
        &p.records,
        &swarmie_durable_tasks::SignalSnapshot::new(),
        11,
        p.sequence,
        vec![
            TaskChange::Checkpoint {
                id,
                attempt,
                revision: 0,
                checkpoint: 40.into(),
            },
            TaskChange::Renew {
                id,
                attempt,
                lease_ms: NonZeroU64::new(10).unwrap(),
            },
            TaskChange::Release { id, attempt },
        ],
    )
    .unwrap();
    let task = &next.records[&id];
    assert_eq!(task.checkpoint, Some(40.into()));
    assert_eq!(task.revision, 1);
    assert_eq!(task.spec.input, serde_json::Value::from(100));
    assert!(task.assignment.is_none());
}
#[test]
fn completion_and_release_commit_together_and_repeated_completion_is_refused() {
    let (p, id, attempt) = initial();
    let next = apply_changes(
        &p.records,
        &swarmie_durable_tasks::SignalSnapshot::new(),
        11,
        p.sequence,
        vec![
            TaskChange::Complete {
                id,
                attempt,
                revision: 0,
                result: 40.into(),
            },
            TaskChange::Release { id, attempt },
        ],
    )
    .unwrap();
    assert_eq!(next.records[&id].status, TaskStatus::Completed(40.into()));
    assert!(next.records[&id].assignment.is_none());
    assert!(matches!(
        apply_changes(
            &next.records,
            &swarmie_durable_tasks::SignalSnapshot::new(),
            12,
            next.sequence,
            vec![acquire(id, AttemptId::new())]
        ),
        Err(Rejection::Terminal)
    ));
}
#[test]
fn typed_failure_survives_serialization() {
    let failure = TaskFailure::from(tembra::Error::new(
        tembra::Code::Unavailable,
        "fixture.offline",
        "try later",
    ));
    let data = serde_json::to_vec(&failure).unwrap();
    let restored: TaskFailure = serde_json::from_slice(&data).unwrap();
    assert_eq!(failure, restored);
}
