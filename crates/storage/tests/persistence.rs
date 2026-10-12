use mpai::*;
use storage::SqliteSessionStore;

fn snapshot(id: &str) -> SessionSnapshot {
    SessionSnapshot {
        working_context: None,
        execution_policy: Default::default(),
        id: id.into(),
        native: NativeSessionRef {
            provider: "external".into(),
            id: "durable-native-id".into(),
        },
        options: SpawnOptions::new("external", "/tmp"),
        capabilities: Capabilities {
            resume: true.into(),
            ..Default::default()
        },
        state: SessionState::Closed,
        sequence: 42,
        turn: None,
        turns_completed: 2,
        metadata: Default::default(),
        queue: Default::default(),
    }
}
fn event(id: &str, sequence: u64) -> RuntimeEvent {
    RuntimeEvent {
        session_id: id.into(),
        sequence,
        turn_id: Some("turn".into()),
        source: "external".into(),
        timestamp_ms: sequence,
        kind: EventKind::TextDelta {
            text: "fragment".into(),
            item_id: Some("message".into()),
        },
        raw: Some(serde_json::json!({"native": true})),
    }
}
fn record(id: &str) -> SubmissionRecord {
    SubmissionRecord {
        receipt: SubmissionReceipt {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: id.into(),
            idempotency_key: "key".into(),
            turn_id: "turn".into(),
            delivery: DeliveryKnowledge::NotSent,
            recovery_required: true,
            outcome: None,
            error: None,
        },
        authored: "authored".into(),
        projected: "projected".into(),
        terminal_native_reason: None,
        terminal_raw: None,
        terminal_sequence: None,
        recovered_messages: vec![],
        reconciliation_raw: None,
    }
}
#[tokio::test]
async fn snapshots_and_store_identity_survive_reopening_with_private_state() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteSessionStore::new(dir.path()).await.unwrap();
    let identity = store.identity().clone();
    let snapshot = snapshot(&uuid::Uuid::new_v4().to_string());
    store.save(&snapshot).await.unwrap();
    drop(store);
    let store = SqliteSessionStore::new(dir.path()).await.unwrap();
    let saved = store.load(&snapshot.id).await.unwrap().unwrap();
    assert_eq!(saved.native.id, snapshot.native.id);
    assert_eq!(saved.sequence, 42);
    assert_eq!(store.identity(), &identity);
    assert_eq!(store.list().await.unwrap().len(), 1);
    assert!(store.load("../../outside").await.is_err());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for path in [
            dir.path().join("settings.json"),
            store.database_path().into(),
            dir.path().join("state/host.sqlite-wal"),
            dir.path().join("state/host.sqlite-shm"),
        ] {
            if path.exists() {
                assert_eq!(
                    std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
                    0o600
                );
            }
        }
    }
}
#[tokio::test]
async fn events_page_reopen_and_reject_duplicate_or_reordered_sequences() {
    let directory = tempfile::tempdir().unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    let store = SqliteSessionStore::new(directory.path()).await.unwrap();
    for sequence in 1..=3 {
        store.append_event(&event(&id, sequence)).await.unwrap();
    }
    assert_eq!(store.events(&id, 1, 1).await.unwrap()[0].sequence, 2);
    drop(store);
    let store = SqliteSessionStore::new(directory.path()).await.unwrap();
    assert_eq!(store.events(&id, 0, 4096).await.unwrap().len(), 3);
    assert!(store.append_event(&event(&id, 3)).await.is_err());
    assert!(store.append_event(&event(&id, 1)).await.is_err());
    store.append_event(&event(&id, 4)).await.unwrap();
    assert_eq!(store.events(&id, 3, 1).await.unwrap()[0].sequence, 4);
    assert!(store.events("../outside", 0, 1).await.is_err());
    assert!(store.events(&id, 0, 0).await.is_err());
    assert!(store.append_event(&event(&id, u64::MAX)).await.is_err());
}
#[tokio::test]
async fn failed_event_insert_rolls_back_snapshot_and_submission_together() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteSessionStore::new(dir.path()).await.unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    let mut snapshot = snapshot(&id);
    snapshot.sequence = 1;
    store
        .commit(SessionWrite {
            snapshot: Some(&snapshot),
            events: &[event(&id, 1)],
            ..Default::default()
        })
        .await
        .unwrap();
    snapshot.metadata.title = Some("must roll back".into());
    let record = record(&id);
    // The duplicate is rejected after the snapshot and receipt writes inside SQLite.
    assert!(
        store
            .commit(SessionWrite {
                snapshot: Some(&snapshot),
                submission: Some(&record),
                events: &[event(&id, 1)]
            })
            .await
            .is_err()
    );
    drop(store);
    let reopened = SqliteSessionStore::new(dir.path()).await.unwrap();
    assert!(
        reopened
            .load(&id)
            .await
            .unwrap()
            .unwrap()
            .metadata
            .title
            .is_none()
    );
    assert!(reopened.submissions(&id).await.unwrap().is_empty());
    assert_eq!(reopened.events(&id, 0, 4096).await.unwrap().len(), 1);
    snapshot.sequence = 2;
    reopened
        .commit(SessionWrite {
            snapshot: Some(&snapshot),
            submission: Some(&record),
            events: &[event(&id, 2)],
        })
        .await
        .unwrap();
    assert_eq!(
        reopened.submissions(&id).await.unwrap()[0].receipt.id,
        record.receipt.id
    );
    assert_eq!(reopened.load(&id).await.unwrap().unwrap().sequence, 2);
}
#[tokio::test]
async fn receipt_identity_and_projected_input_cannot_be_replaced() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteSessionStore::new(dir.path()).await.unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    let mut record = record(&id);
    store.save_submission(&record).await.unwrap();
    record.receipt.delivery = DeliveryKnowledge::Accepted;
    store.save_submission(&record).await.unwrap();
    record.projected = "changed".into();
    assert!(store.save_submission(&record).await.is_err());
    let retained = store.submissions(&id).await.unwrap();
    assert_eq!(retained[0].projected.text().unwrap(), "projected");
    assert_eq!(retained[0].receipt.delivery, DeliveryKnowledge::Accepted);
    assert!(
        store
            .commit(SessionWrite {
                snapshot: Some(&snapshot(&id)),
                events: &[event(&uuid::Uuid::new_v4().to_string(), 1)],
                ..Default::default()
            })
            .await
            .is_err()
    );
}
#[tokio::test]
async fn configured_missing_replaced_or_unsupported_store_never_initializes_empty_history() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteSessionStore::new(dir.path()).await.unwrap();
    let database = store.database_path().to_owned();
    std::fs::rename(&database, dir.path().join("retained.sqlite")).unwrap();
    assert!(store.list().await.is_err());
    assert!(SqliteSessionStore::new(dir.path()).await.is_err());
    assert!(!database.exists());
    std::fs::rename(dir.path().join("retained.sqlite"), &database).unwrap();
    drop(store);
    let connection = rusqlite::Connection::open(&database).unwrap();
    connection.pragma_update(None, "user_version", 99).unwrap();
    drop(connection);
    assert!(SqliteSessionStore::new(dir.path()).await.is_err());
    assert!(database.exists());
}
#[tokio::test]
async fn legacy_files_are_preserved_and_require_a_separate_fresh_root() {
    let dir = tempfile::tempdir().unwrap();
    let old_file = dir.path().join(format!("{}.json", uuid::Uuid::new_v4()));
    std::fs::write(&old_file, b"old development snapshot").unwrap();
    assert!(SqliteSessionStore::new(dir.path()).await.is_err());
    assert_eq!(
        std::fs::read(old_file).unwrap(),
        b"old development snapshot"
    );
    assert!(!dir.path().join("state/host.sqlite").exists());
}
#[tokio::test]
async fn concurrent_bootstrap_retains_one_identity_and_other_roots_get_distinct_identities() {
    let dir = tempfile::tempdir().unwrap();
    let (first, second) = tokio::join!(
        SqliteSessionStore::new(dir.path()),
        SqliteSessionStore::new(dir.path())
    );
    assert_eq!(first.unwrap().identity(), second.unwrap().identity());
    let other = tempfile::tempdir().unwrap();
    assert_ne!(
        SqliteSessionStore::new(dir.path())
            .await
            .unwrap()
            .identity(),
        SqliteSessionStore::new(other.path())
            .await
            .unwrap()
            .identity()
    );
}
#[tokio::test]
#[ignore = "hardware-dependent diagnostic, run explicitly with --ignored --nocapture"]
async fn durable_journal_burst_throughput() {
    let directory = tempfile::tempdir().unwrap();
    let store = SqliteSessionStore::new(directory.path()).await.unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    let started = std::time::Instant::now();
    for sequence in 1..=200 {
        let mut event = event(&id, sequence);
        event.raw = Some(serde_json::json!({"payload": "x".repeat(2048)}));
        store.append_event(&event).await.unwrap();
    }
    println!("200 durable 2 KiB events: {:?}", started.elapsed());
    assert_eq!(store.events(&id, 0, 4096).await.unwrap().len(), 200);
}
