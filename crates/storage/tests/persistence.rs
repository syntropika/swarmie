use mpai::*;
use storage::FileSessionStore;
#[tokio::test]
async fn snapshots_survive_reopening_and_reject_path_traversal() {
    let dir = tempfile::tempdir().unwrap();
    let store = FileSessionStore::new(dir.path()).await.unwrap();
    let snapshot = SessionSnapshot {
        id: uuid::Uuid::new_v4().to_string(),
        native: NativeSessionRef {
            provider: "external".into(),
            id: "durable-native-id".into(),
        },
        options: SpawnOptions::new("external", "/tmp"),
        capabilities: Capabilities {
            resume: true,
            ..Default::default()
        },
        state: SessionState::Closed,
        sequence: 42,
        turn: None,
        turns_completed: 2,
        metadata: Default::default(),
        queue: Default::default(),
    };
    store.save(&snapshot).await.unwrap();
    drop(store);
    let store = FileSessionStore::new(dir.path()).await.unwrap();
    let saved = store.load(&snapshot.id).await.unwrap().unwrap();
    assert_eq!(saved.native.id, snapshot.native.id);
    assert_eq!(saved.sequence, 42);
    assert_eq!(store.list().await.unwrap().len(), 1);
    assert!(store.load("../../outside").await.is_err());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(dir.path().join(format!("{}.json", snapshot.id)))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
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
        raw: Some(serde_json::json!({"native":true})),
    }
}
#[tokio::test]
async fn transcripts_page_reopen_and_recover_only_an_incomplete_final_write() {
    let directory = tempfile::tempdir().unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    let store = FileSessionStore::new(directory.path()).await.unwrap();
    for sequence in 1..=3 {
        store.append_event(&event(&id, sequence)).await.unwrap();
    }
    assert_eq!(store.events(&id, 1, 1).await.unwrap()[0].sequence, 2);
    drop(store);
    let journal = directory.path().join(format!("{id}.jsonl"));
    use std::io::Write;
    std::fs::OpenOptions::new()
        .append(true)
        .open(&journal)
        .unwrap()
        .write_all(b"{\"sequence\":4")
        .unwrap();
    let store = FileSessionStore::new(directory.path()).await.unwrap();
    assert_eq!(store.events(&id, 0, 4096).await.unwrap().len(), 3);
    store.append_event(&event(&id, 4)).await.unwrap();
    assert_eq!(store.events(&id, 0, 4096).await.unwrap().len(), 4);
    assert_eq!(store.events(&id, 3, 1).await.unwrap()[0].sequence, 4);
    assert!(store.events("../outside", 0, 1).await.is_err());
    std::fs::OpenOptions::new()
        .append(true)
        .open(&journal)
        .unwrap()
        .write_all(b"{malformed}\n")
        .unwrap();
    assert!(
        store.events(&id, 0, 4096).await.is_err(),
        "complete corrupt records must never be silently dropped"
    );
}
#[tokio::test]
#[ignore = "hardware-dependent diagnostic, run explicitly with --ignored --nocapture"]
async fn durable_journal_burst_throughput() {
    let directory = tempfile::tempdir().unwrap();
    let store = FileSessionStore::new(directory.path()).await.unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    let started = std::time::Instant::now();
    for sequence in 1..=200 {
        let mut event = event(&id, sequence);
        event.raw = Some(serde_json::json!({"payload":"x".repeat(2048)}));
        store.append_event(&event).await.unwrap();
    }
    let elapsed = started.elapsed();
    println!(
        "200 durable 2 KiB events: {elapsed:?}, {:.0} events/s",
        200.0 / elapsed.as_secs_f64()
    );
    assert_eq!(store.events(&id, 0, 4096).await.unwrap().len(), 200);
}
