use std::{
    collections::BTreeMap,
    num::{NonZeroU32, NonZeroU64},
};
use swarmie_durable_tasks::*;

fn initial() -> (PreparedTasks, TaskId, AttemptId) {
    let id = TaskId::new();
    let attempt = AttemptId::new();
    let prepared = apply_changes(
        &BTreeMap::new(),
        &SignalSnapshot::new(),
        10,
        0,
        vec![
            TaskChange::Create {
                spec: TaskSpec {
                    id,
                    definition: TaskDefinition::new("receipt.task", NonZeroU32::new(1).unwrap())
                        .unwrap(),
                    input: 0.into(),
                },
                principal: "alice".into(),
            },
            TaskChange::Acquire {
                id,
                attempt,
                revision: 0,
                executor: ExecutorId::new(),
                lease_ms: NonZeroU64::new(100).unwrap(),
            },
        ],
    )
    .unwrap();
    (prepared, id, attempt)
}
fn signal(id: TaskId, key: SignalKey, value: u64) -> TaskChange {
    TaskChange::Signal {
        id,
        key,
        value: value.into(),
    }
}
fn wait(id: TaskId, attempt: AttemptId, key: SignalKey) -> TaskChange {
    TaskChange::Suspend {
        id,
        attempt,
        revision: 0,
        checkpoint: 42.into(),
        condition: WaitFor::Signal(key),
    }
}
#[test]
fn unloaded_signal_state_is_refused_before_any_change() {
    let (p, id, attempt) = initial();
    let key = SignalKey::new();
    let before = p.records.clone();
    for changes in [
        vec![signal(id, key, 7)],
        vec![wait(id, attempt, key)],
        vec![
            TaskChange::Checkpoint {
                id,
                attempt,
                revision: 0,
                checkpoint: 1.into(),
            },
            signal(id, key, 7),
        ],
    ] {
        assert!(matches!(
            apply_changes(&p.records, &SignalSnapshot::new(), 11, p.sequence, changes),
            Err(Rejection::IncompleteReceipts)
        ));
        assert_eq!(p.records, before);
    }
}
#[test]
fn confirmed_absence_and_loaded_early_receipt_are_distinct() {
    let (p, id, attempt) = initial();
    let key = SignalKey::new();
    let absent = SignalSnapshot::from([((id, key), None)]);
    let signalled = apply_changes(
        &p.records,
        &absent,
        11,
        p.sequence,
        vec![signal(id, key, 7)],
    )
    .unwrap();
    let loaded = SignalSnapshot::from([((id, key), signalled.signals.get(&(id, key)).cloned())]);
    let ready = apply_changes(
        &signalled.records,
        &loaded,
        12,
        signalled.sequence,
        vec![wait(id, attempt, key)],
    )
    .unwrap();
    assert_eq!(ready.records[&id].status, TaskStatus::Ready);
    assert_eq!(
        ready.records[&id].delivery.as_ref().unwrap().value,
        serde_json::json!(7)
    );
    assert!(ready.signals[&(id, key)].consumed);
}
#[test]
fn required_keys_are_unique_and_partial_snapshots_are_refused() {
    let (p, id, _) = initial();
    let (one, two) = (SignalKey::new(), SignalKey::new());
    let changes = vec![signal(id, one, 1), signal(id, two, 2), signal(id, one, 1)];
    assert_eq!(required_receipts(&changes).len(), 2);
    assert!(matches!(
        apply_changes(
            &p.records,
            &SignalSnapshot::from([((id, one), None)]),
            11,
            p.sequence,
            changes
        ),
        Err(Rejection::IncompleteReceipts)
    ));
}
#[test]
fn terminal_duplicates_retain_conflict_and_consumption_evidence() {
    let (p, id, attempt) = initial();
    let key = SignalKey::new();
    let p = apply_changes(
        &p.records,
        &SignalSnapshot::from([((id, key), None)]),
        11,
        p.sequence,
        vec![
            signal(id, key, 7),
            wait(id, attempt, key),
            TaskChange::Complete {
                id,
                attempt,
                revision: 1,
                result: 9.into(),
            },
            TaskChange::Release { id, attempt },
        ],
    )
    .unwrap();
    let snapshot = SignalSnapshot::from([((id, key), Some(p.signals[&(id, key)].clone()))]);
    let duplicate = apply_changes(
        &p.records,
        &snapshot,
        12,
        p.sequence,
        vec![signal(id, key, 7)],
    )
    .unwrap();
    assert_eq!(duplicate.records, p.records);
    assert_eq!(duplicate.signals, p.signals);
    assert!(matches!(
        apply_changes(
            &p.records,
            &snapshot,
            12,
            p.sequence,
            vec![signal(id, key, 8)]
        ),
        Err(Rejection::Invalid)
    ));
}
