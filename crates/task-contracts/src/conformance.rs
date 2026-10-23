//! Reusable backend conformance through the public TaskStorage Contract.
//!
//! Run against a fresh provider. The suite has no concrete backend dependencies and leaves
//! committed records available for a backend-specific restart or process-crash fixture.
//! The Reference must be bound to a live Context from an installed Facet-owned root, so
//! consumer operations exercise the same authenticated caller boundary as real plugins.

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
        provenance: None,
        expected_revision: None,
        record,
    }
}

fn replace(mut record: TaskRecord) -> TaskWrite {
    let revision = record.revision;
    record.revision += 1;
    TaskWrite {
        provenance: None,
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
/// bounded consistent pagination, explicit owner admission and stable store identity using only
/// public References.
///
/// # Errors
/// Propagates unexpected provider errors. A failed guarantee panics with the invariant violated.
pub async fn run(storage: &TaskStorageRef) -> Result<()> {
    // Identity is answered before any scheduler owner exists and never changes afterwards.
    let identity = storage.identity().await?;
    assert!(!identity.store_id.is_empty());
    assert_eq!(storage.identity().await?, identity, "identity is stable");
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
                    provenance: None,
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
                    provenance: None,
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
    assert_eq!(cursor.store_id, identity.store_id);
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
    large_payloads(storage).await?;
    outbox(storage, &identity).await?;
    storage
        .release_owner(StorageOwner { id: OWNER.into() })
        .await?;
    assert_eq!(
        storage.identity().await?,
        identity,
        "identity remains available after ownership is released"
    );
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
        .await?;
    assert_eq!(storage.identity().await?, identity);
    Ok(())
}

async fn large_payloads(storage: &TaskStorageRef) -> Result<()> {
    let mut expected = std::collections::BTreeSet::new();
    for id in ["large-a", "large-b"] {
        let mut record = task(id);
        record.checkpoint = serde_json::json!({"data":"x".repeat(600_000)});
        let (_, records) = commit(storage, vec![create(record)]).await?;
        assert!(read(storage, id).await? == records[0]);
        expected.insert(id.to_owned());
    }
    let mut query = TaskQuery {
        limit: 256,
        ..Default::default()
    };
    let mut seen = std::collections::BTreeSet::new();
    let mut pages = 0;
    loop {
        let page = storage.scan(query.clone()).await?;
        assert!(crate::task_wire_bytes(&page)? <= crate::MAX_TASK_PAGE_BYTES);
        pages += 1;
        for record in page.records {
            if record.id.starts_with("large-") {
                assert!(seen.insert(record.id));
            }
        }
        match page.next {
            Some(cursor) => query.cursor = Some(cursor),
            None => break,
        }
    }
    assert_eq!(seen, expected);
    assert!(
        pages >= 2,
        "large records require byte-bounded continuation pages"
    );
    let before = storage
        .scan(TaskQuery {
            limit: 1,
            ..Default::default()
        })
        .await?
        .sequence;
    let original = read(storage, "large-a").await?;
    let mut oversized = original.clone();
    oversized.checkpoint = serde_json::json!({"data":"x".repeat(crate::MAX_TASK_RECORD_BYTES)});
    let error = storage
        .commit(TaskTransaction {
            owner: OWNER.into(),
            writes: vec![replace(oversized)],
        })
        .await
        .unwrap_err();
    assert_eq!(error.reason, "task_storage_too_large");
    assert!(read(storage, "large-a").await? == original);
    let writes = ["large-batch-a", "large-batch-b"]
        .into_iter()
        .map(|id| {
            let mut record = task(id);
            record.checkpoint = serde_json::json!({"data":"x".repeat(520_000)});
            create(record)
        })
        .collect();
    let batch = TaskTransaction {
        owner: OWNER.into(),
        writes,
    };
    assert!(crate::task_wire_bytes(&batch)? > crate::MAX_TASK_PAGE_BYTES);
    assert!(crate::task_wire_bytes(&batch)? < 1_048_576);
    assert_eq!(
        storage.commit(batch).await.unwrap_err().reason,
        "task_storage_too_large"
    );
    assert!(
        storage
            .get(TaskLookup {
                id: "large-batch-a".into()
            })
            .await?
            .is_none()
    );
    assert_eq!(
        storage
            .scan(TaskQuery {
                limit: 1,
                ..Default::default()
            })
            .await?
            .sequence,
        before
    );
    let mut record = task("control-reserve");
    record.attempts = 9;
    record.restarts = 9;
    // Measure the exact allowed non-control boundary with worst-case mutable metadata.
    let mut widest = tembra::contracts::to_portable_value(&record)
        .map_err(|error| RuntimeError::invalid("conformance_encoding", error.to_string()))?;
    let fields = widest.as_object_mut().expect("record object");
    fields.remove("outcome");
    fields.remove("blocking_reason");
    fields.insert("revision".into(), crate::MAX_PORTABLE_INTEGER.into());
    for field in ["attempts", "restarts", "definition_version"] {
        fields.insert(field.into(), u32::MAX.into());
    }
    fields.insert("status".into(), "completing".into());
    fields.insert("checkpoint".into(), serde_json::json!(""));
    let available = crate::MAX_TASK_RECORD_BYTES
        - crate::MAX_TASK_CONTROL_BYTES
        - crate::task_wire_bytes(&widest)?;
    record.checkpoint = serde_json::json!("x".repeat(available));
    let (_, records) = commit(storage, vec![create(record)]).await?;
    let mut record = records[0].clone();
    record.attempts = 10;
    record.restarts = 10;
    record.status = TaskStatus::Terminal;
    record.outcome = Some(TaskOutcome::Faulted {
        error: "diagnostic".repeat(400),
    });
    commit(storage, vec![replace(record)]).await?;
    assert!(read(storage, "control-reserve").await?.is_terminal());
    Ok(())
}

/// Reusable provider requirements for structural atomic outbox, append-stable pagination,
/// delivered-prefix acknowledgement, multi-consumer retention, and CAS absence. Every reported
/// outbox identity equals the provider's store identity.
async fn outbox(storage: &TaskStorageRef, identity: &crate::TaskStorageIdentity) -> Result<()> {
    use crate::{
        TaskOutboxAcknowledgement, TaskOutboxCompaction, TaskOutboxConsumer, TaskOutboxQuery,
    };
    let initial = storage.outbox_status().await?;
    assert_eq!(initial.store_id, identity.store_id);
    initial.limits.validate()?;
    assert!(initial.consumers.is_empty());
    assert_eq!(initial.compactable_events, 0);
    assert_eq!(initial.pending_events, initial.retained_events);
    assert!(crate::task_wire_bytes(&initial)? <= crate::MAX_TASK_PAGE_BYTES);
    assert_eq!(
        storage.outbox_status().await?,
        initial,
        "status is read-only"
    );
    let fast = "conformance-outbox-fast";
    let slow = "conformance-outbox-offline";
    let state = storage
        .register_outbox_consumer(TaskOutboxConsumer { id: fast.into() })
        .await?;
    let other = storage
        .register_outbox_consumer(TaskOutboxConsumer { id: slow.into() })
        .await?;
    assert_eq!(state.store_id, other.store_id);
    assert_eq!(state.store_id, identity.store_id);
    let owner = state.owner_facet.as_ref().expect("managed consumer owner");
    crate::validate_task_outbox_owner_facet(owner)?;
    assert_eq!(state.owner_facet, other.owner_facet);
    let registered = storage.outbox_status().await?;
    assert_eq!(registered.store_id, state.store_id);
    assert_eq!(registered.consumers.len(), 2);
    assert_eq!(registered.retained_events, initial.retained_events);
    assert_eq!(registered.retained_bytes, initial.retained_bytes);
    let before = storage
        .compact_outbox(TaskOutboxCompaction { limit: 4096 })
        .await?;
    assert_eq!(before.removed, 0, "unacknowledged history is retained");
    let mut all = Vec::new();
    let mut after = None;
    loop {
        let page = storage
            .read_outbox(TaskOutboxQuery {
                consumer_id: fast.into(),
                after: after.clone(),
                limit: 2,
            })
            .await?;
        assert_eq!(page.store_id, state.store_id);
        assert!(crate::task_wire_bytes(&page)? <= crate::MAX_TASK_PAGE_BYTES);
        if page.events.is_empty() {
            break;
        }
        after = page.next;
        all.extend(page.events);
    }
    assert!(!all.is_empty());
    assert!(
        all.iter()
            .all(|event| event.id.store_id == identity.store_id)
    );
    for pair in all.windows(2) {
        assert!(
            (pair[0].id.sequence, pair[0].id.ordinal) < (pair[1].id.sequence, pair[1].id.ordinal)
        );
    }
    let replay = storage
        .read_outbox(TaskOutboxQuery {
            consumer_id: fast.into(),
            after: None,
            limit: 1,
        })
        .await?;
    assert_eq!(
        replay.events[0].id, all[0].id,
        "read does not imply acknowledgement"
    );
    let tail = after.expect("existing outbox tail");
    let mut future = tail.clone();
    future.sequence += 1;
    assert!(
        storage
            .acknowledge_outbox(TaskOutboxAcknowledgement {
                consumer_id: fast.into(),
                through: future.clone()
            })
            .await
            .is_err()
    );
    assert!(
        storage
            .read_outbox(TaskOutboxQuery {
                consumer_id: fast.into(),
                after: Some(future),
                limit: 1
            })
            .await
            .is_err()
    );
    let conflict = storage
        .commit(TaskTransaction {
            owner: OWNER.into(),
            writes: vec![
                create(task("parent")),
                create(task("outbox-conflict-never-created")),
            ],
        })
        .await?;
    assert!(matches!(conflict, CommitResult::Conflict { .. }));
    assert!(
        storage
            .read_outbox(TaskOutboxQuery {
                consumer_id: fast.into(),
                after: Some(tail.clone()),
                limit: 256
            })
            .await?
            .events
            .is_empty()
    );
    let mut child = task("outbox-child");
    child.parent = Some("outbox-parent".into());
    let (sequence, _) = commit(storage, vec![create(task("outbox-parent")), create(child)]).await?;
    let appended = storage
        .read_outbox(TaskOutboxQuery {
            consumer_id: fast.into(),
            after: Some(tail),
            limit: 256,
        })
        .await?;
    assert_eq!(appended.events.len(), 2);
    assert_eq!(appended.events[0].task_id, "outbox-parent");
    assert_eq!(appended.events[1].parent.as_deref(), Some("outbox-parent"));
    for (ordinal, event) in appended.events.iter().enumerate() {
        assert_eq!(event.id.sequence, sequence);
        assert_eq!(event.id.ordinal, ordinal as u32);
        assert_eq!(event.previous_status, None);
        assert_eq!(event.current_status, TaskStatus::Pending);
    }
    let through = appended.next.expect("tail");
    storage
        .acknowledge_outbox(TaskOutboxAcknowledgement {
            consumer_id: fast.into(),
            through: through.clone(),
        })
        .await?;
    storage
        .acknowledge_outbox(TaskOutboxAcknowledgement {
            consumer_id: fast.into(),
            through,
        })
        .await?;
    assert_eq!(
        storage
            .compact_outbox(TaskOutboxCompaction { limit: 4096 })
            .await?
            .removed,
        0,
        "offline consumer protects pending events"
    );
    let recovered = storage
        .read_outbox(TaskOutboxQuery {
            consumer_id: slow.into(),
            after: None,
            limit: 256,
        })
        .await?;
    assert!(!recovered.events.is_empty());
    assert_eq!(recovered.events[0].id, all[0].id);
    storage
        .remove_outbox_consumer(TaskOutboxConsumer { id: slow.into() })
        .await?;
    let releasable = storage.outbox_status().await?;
    assert_eq!(releasable.pending_events, 0);
    assert_eq!(releasable.compactable_events, releasable.retained_events);
    assert!(
        storage
            .compact_outbox(TaskOutboxCompaction { limit: 4096 })
            .await?
            .removed
            > 0
    );
    let replacement = storage
        .register_outbox_consumer(TaskOutboxConsumer {
            id: "conformance-outbox-new".into(),
        })
        .await?;
    assert!(replacement.retained_after.is_some());
    assert_eq!(replacement.acknowledged, replacement.retained_after);
    assert!(
        storage
            .read_outbox(TaskOutboxQuery {
                consumer_id: replacement.consumer_id.clone(),
                after: None,
                limit: 256
            })
            .await?
            .events
            .is_empty()
    );
    storage
        .remove_outbox_consumer(TaskOutboxConsumer { id: fast.into() })
        .await?;
    storage
        .remove_outbox_consumer(TaskOutboxConsumer {
            id: replacement.consumer_id,
        })
        .await?;
    Ok(())
}
