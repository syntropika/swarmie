//! Regressions for conversation isolation, reactive failure isolation and SQLite admission.
mod support;

use serde_json::json;
use support::*;
use swarmie_session_contracts::*;
use tembra::prelude::*;

#[test]
fn conversation_filter_isolates_model_context_and_binds_continuations() -> Result<()> {
    let directory = tempfile::tempdir().unwrap();
    run(|host, driver| async move {
        let store = open(
            &host,
            &directory.path().join("sessions.sqlite"),
            driver.clone(),
        )
        .await?;
        create(&store, "shared").await?;
        let mut events: Vec<_> = (0..6)
            .map(|n| {
                let mut value =
                    message(format!("m{n}"), "user", &format!("conversation:{}", n % 2));
                value.correlation.conversation_id = Some(format!("conversation:{}", n % 2));
                value.correlation.turn_id = Some("same-turn-name".into());
                value
            })
            .collect();
        events.push(message("unscoped", "user", "No conversation"));
        store.append(batch("shared", "interleaved", events)).await?;
        for kind in [ProjectionKind::Conversation, ProjectionKind::Trajectory] {
            let mut source = EventQuery::new("shared");
            source.conversation_id = Some("conversation:1".into());
            let page = store.project(ProjectionQuery { source, kind }).await?;
            assert_eq!(
                page.entries
                    .iter()
                    .map(|event| event.sequence)
                    .collect::<Vec<_>>(),
                vec![2, 4, 6]
            );
        }
        assert_eq!(
            store.events(EventQuery::new("shared")).await?.events.len(),
            7
        );
        let mut encoded = serde_json::to_value(EventQuery::new("shared")).unwrap();
        encoded["conversation_id"] = json!("conversation:0");
        encoded["turn_id"] = json!("same-turn-name");
        encoded["limit"] = json!(1);
        let mut query: EventQuery = serde_json::from_value(encoded)
            .expect("conversation filter is a supported source selector");
        let first = store
            .project(ProjectionQuery {
                source: query.clone(),
                kind: ProjectionKind::ModelContext,
            })
            .await?;
        assert_eq!(first.entries[0].event.payload["content"], "conversation:0");
        let continuation = first
            .next
            .clone()
            .expect("remaining entries in this conversation");
        let mut collected = first.entries;
        query.cursor = Some(continuation.clone());
        loop {
            let page = store
                .project(ProjectionQuery {
                    source: query.clone(),
                    kind: ProjectionKind::ModelContext,
                })
                .await?;
            collected.extend(page.entries);
            query.cursor = page.next;
            if query.cursor.is_none() {
                break;
            }
        }
        assert_eq!(
            collected
                .iter()
                .map(|entry| entry.sequence)
                .collect::<Vec<_>>(),
            vec![1, 3, 5]
        );
        assert!(
            collected
                .iter()
                .all(|entry| entry.event.correlation.conversation_id.as_deref()
                    == Some("conversation:0"))
        );
        let mut swapped = serde_json::to_value(&query).unwrap();
        swapped["conversation_id"] = json!("conversation:1");
        swapped["cursor"] = serde_json::to_value(continuation).unwrap();
        assert_eq!(
            store
                .events(serde_json::from_value(swapped).unwrap())
                .await
                .unwrap_err()
                .reason,
            "session_store_cursor"
        );
        host.close().await
    })
}

#[test]
fn incompatible_database_is_rejected_before_journal_mode_changes() -> Result<()> {
    for schema in ["foreign", "future"] {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("other.sqlite");
        let raw = rusqlite::Connection::open(&database).unwrap();
        raw.execute_batch("PRAGMA journal_mode=DELETE; CREATE TABLE owner_data(value TEXT); INSERT INTO owner_data VALUES('keep');").unwrap();
        if schema == "future" {
            raw.execute_batch("PRAGMA application_id=1398231877; PRAGMA user_version=999;")
                .unwrap();
        }
        drop(raw);
        run(|host, driver| async move {
            assert!(open(&host, &database, driver.clone()).await.is_err());
            let raw = rusqlite::Connection::open(&database).unwrap();
            let mode: String = raw
                .pragma_query_value(None, "journal_mode", |row| row.get(0))
                .unwrap();
            assert_eq!(
                mode.to_lowercase(),
                "delete",
                "refusing {schema} schema must not adopt WAL"
            );
            assert_eq!(
                raw.query_row("SELECT value FROM owner_data", [], |row| row
                    .get::<_, String>(0))
                    .unwrap(),
                "keep"
            );
            host.close().await
        })?;
    }
    Ok(())
}

#[test]
fn reopened_history_keeps_conversation_indexes_and_immutable_receipts() -> Result<()> {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("sessions.sqlite");
    let database = &database;
    let mut events = vec![message("a", "user", "A"), message("b", "assistant", "B")];
    for (index, event) in events.iter_mut().enumerate() {
        event.correlation.conversation_id = Some(format!("conversation:{index}"));
        event.correlation.turn_id = Some("turn".into());
    }
    let request = batch("shared", "original", events);
    let first_request = request.clone();
    run(|host, driver| async move {
        let store = open(&host, database, driver).await?;
        create(&store, "shared").await?;
        store.append(first_request).await?;
        host.close().await
    })?;
    let raw = rusqlite::Connection::open(database).unwrap();
    let before: Vec<String> = raw
        .prepare("SELECT event FROM session_events ORDER BY sequence")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(|row| row.unwrap())
        .collect();
    let receipt: String = raw
        .query_row("SELECT receipt FROM session_batches", [], |row| row.get(0))
        .unwrap();
    drop(raw);
    run(|host, driver| async move {
        let store = open(&host, database, driver).await?;
        let mut query = EventQuery::new("shared");
        query.conversation_id = Some("conversation:0".into());
        query.turn_id = Some("turn".into());
        assert_eq!(store.events(query).await?.events[0].event.event_id, "a");
        assert_eq!(canonical(&store.append(request).await?)?, receipt);
        let raw = rusqlite::Connection::open(database).unwrap();
        let after: Vec<String> = raw
            .prepare("SELECT event FROM session_events ORDER BY sequence")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(|row| row.unwrap())
            .collect();
        assert_eq!(after, before);
        assert_eq!(
            raw.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            2
        );
        let plan: String = raw.query_row("EXPLAIN QUERY PLAN SELECT sequence FROM session_events WHERE session_id='shared' AND conversation_id='conversation:0' AND turn_id='turn' ORDER BY sequence LIMIT 128", [], |row| row.get(3)).unwrap();
        assert!(plan.contains("session_events_conversation_turn"), "{plan}");
        assert!(
            raw.execute("UPDATE session_events SET conversation_id='different'", [])
                .is_err()
        );
        host.close().await
    })
}

#[test]
fn unsupported_schema_version_preserves_history_and_journal_mode() -> Result<()> {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("sessions.sqlite");
    run(|host, driver| {
        let database = &database;
        async move {
            let store = open(&host, database, driver).await?;
            create(&store, "retained").await?;
            store
                .append(batch(
                    "retained",
                    "input",
                    vec![message("input", "user", "Keep this history")],
                ))
                .await?;
            host.close().await
        }
    })?;
    let raw = rusqlite::Connection::open(&database).unwrap();
    raw.execute_batch("PRAGMA journal_mode=DELETE; PRAGMA user_version=1;")
        .unwrap();
    drop(raw);
    let before = std::fs::read(&database).unwrap();
    run(|host, driver| {
        let database = &database;
        async move {
            let error = match open(&host, database, driver).await {
                Ok(_) => panic!("unsupported storage format was admitted"),
                Err(error) => error,
            };
            assert_eq!(error.reason, "session_store_schema");
            host.close().await
        }
    })?;
    assert_eq!(std::fs::read(&database).unwrap(), before);
    let raw = rusqlite::Connection::open(&database).unwrap();
    assert_eq!(
        raw.pragma_query_value(None, "journal_mode", |row| row.get::<_, String>(0))
            .unwrap(),
        "delete"
    );
    assert_eq!(
        raw.query_row("SELECT count(*) FROM session_events", [], |row| row
            .get::<_, u32>(0))
            .unwrap(),
        1
    );
    Ok(())
}
