mod support;

use std::{collections::BTreeMap, path::Path};

use rusqlite::Connection;
use swarmie_session_contracts::*;
use swarmie_task_contracts::*;
use swarmie_task_session_contracts::*;
use tembra::prelude::*;

use support::*;

fn audit(path: &Path) -> Connection {
    let connection = Connection::open(path.join("tasks.sqlite")).unwrap();
    connection.execute_batch("CREATE TABLE ack_audit(sequence INTEGER NOT NULL,ordinal INTEGER NOT NULL); CREATE TRIGGER audit_ack AFTER UPDATE OF ack_sequence,ack_ordinal ON task_outbox_consumers BEGIN INSERT INTO ack_audit VALUES(NEW.ack_sequence,NEW.ack_ordinal); END;").unwrap();
    connection
}

fn acknowledgements(connection: &Connection) -> Vec<(u64, u32)> {
    connection
        .prepare("SELECT sequence,ordinal FROM ack_audit ORDER BY rowid")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

fn requests(path: &Path) -> BTreeMap<String, String> {
    Connection::open(path.join("sessions.sqlite"))
        .unwrap()
        .prepare("SELECT batch_id,request FROM session_batches ORDER BY batch_id")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

async fn routed(harness: &Harness, id: &str) -> Result<()> {
    let request = harness.request(id, "chat", id).await?;
    harness.bindings.prepare(request.clone()).await?;
    harness.bindings.admit(key(&request)).await?;
    Ok(())
}

async fn exclude(harness: &Harness, id: &str) -> Result<()> {
    harness
        .bindings
        .exclude(ExcludedBinding {
            key: BindingKey {
                task_store_id: harness.configuration.task_store_id.clone(),
                task_id: id.into(),
            },
            reason: "explicit unrelated fixture policy".into(),
        })
        .await?;
    Ok(())
}

#[test]
fn one_durable_ack_per_bounded_page_preserves_single_fact_batches() -> Result<()> {
    let directory = tempfile::tempdir().unwrap();
    run(|driver| async move {
        let h = Harness::open(directory.path(), driver, TaskOutboxLimits::default(), false).await?;
        for id in ["a", "b", "c"] {
            routed(&h, id).await?;
        }
        let audit = audit(directory.path());
        let first = h.publisher.publish(PublishRequest { limit: 2 }).await?;
        assert_eq!(first.published, 2);
        assert_eq!(first.excluded, 0);
        let through = first.consumer.acknowledged.unwrap();
        assert_eq!(
            acknowledgements(&audit),
            [(through.sequence, through.ordinal)]
        );
        let mut passes = 1;
        loop {
            let receipt = h.publisher.publish(PublishRequest { limit: 2 }).await?;
            if receipt.published == 0 {
                break;
            }
            passes += 1;
            assert_eq!(acknowledgements(&audit).len(), passes);
        }
        assert_eq!(
            acknowledgements(&audit).len(),
            passes,
            "empty passes never ACK"
        );
        let events = h.sessions.events(EventQuery::new("chat")).await?.events;
        let batches = requests(directory.path());
        assert_eq!(batches.len(), events.len());
        for encoded in batches.values() {
            let request: AppendEvents = serde_json::from_str(encoded).unwrap();
            assert_eq!(
                request.events.len(),
                1,
                "mapping remains one immutable batch per fact"
            );
            assert_eq!(request.expected_head, None);
            let fact: TaskOutboxEvent =
                serde_json::from_value(request.events[0].payload["source"].clone()).unwrap();
            let source = canonical(&fact.id)?;
            use sha2::{Digest, Sha256};
            let identity = |domain: &str| {
                let mut digest = Sha256::new();
                digest.update(domain.as_bytes());
                digest.update([0]);
                digest.update(source.as_bytes());
                format!("task-{}", hex::encode(digest.finalize()))
            };
            assert_eq!(request.batch_id, identity("swarmie.task-session.batch.v1"));
            assert_eq!(
                request.events[0].event_id,
                identity("swarmie.task-session.event.v1")
            );
        }
        h.host.close().await
    })
}

#[test]
fn unresolved_fact_keeps_the_confirmed_and_excluded_prefix_replayable() -> Result<()> {
    let directory = tempfile::tempdir().unwrap();
    run(|driver| async move {
        let h = Harness::open(directory.path(), driver, TaskOutboxLimits::default(), false).await?;
        h.tasks.create(spec("excluded")).await?;
        exclude(&h, "excluded").await?;
        routed(&h, "first").await?;
        h.tasks.create(spec("missing")).await?;
        routed(&h, "later").await?;
        let audit = audit(directory.path());
        for _ in 0..2 {
            assert_eq!(
                h.publisher
                    .publish(PublishRequest { limit: 256 })
                    .await
                    .unwrap_err()
                    .reason,
                "binding_missing"
            );
            assert!(acknowledgements(&audit).is_empty());
            assert!(
                h.storage.outbox_status().await?.consumers[0]
                    .acknowledged
                    .is_none()
            );
            let page = h.sessions.events(EventQuery::new("chat")).await?;
            assert!(
                page.events
                    .iter()
                    .any(|event| event.event.correlation.task_id.as_deref() == Some("first"))
            );
            assert!(
                !page
                    .events
                    .iter()
                    .any(|event| event.event.correlation.task_id.as_deref() == Some("later"))
            );
        }
        let before = requests(directory.path());
        exclude(&h, "missing").await?;
        let receipt = h.publisher.publish(PublishRequest { limit: 256 }).await?;
        assert!(receipt.excluded > 0);
        assert_eq!(acknowledgements(&audit).len(), 1);
        let after = requests(directory.path());
        assert!(
            before
                .iter()
                .all(|(id, request)| after.get(id) == Some(request))
        );
        let page = h.sessions.events(EventQuery::new("chat")).await?;
        assert!(
            page.events
                .iter()
                .any(|event| event.event.correlation.task_id.as_deref() == Some("later"))
        );
        assert!(page.events.iter().all(|event| !matches!(
            event.event.correlation.task_id.as_deref(),
            Some("excluded" | "missing")
        )));
        assert_eq!(drain(&h.publisher).await?, 0);
        h.host.close().await
    })
}

#[test]
fn append_success_ack_failure_replays_exact_batches_across_different_page_sizes() -> Result<()> {
    let directory = tempfile::tempdir().unwrap();
    run(|driver| async move {
        let h = Harness::open(directory.path(), driver, TaskOutboxLimits::default(), false).await?;
        for id in ["a", "b", "c"] {
            routed(&h, id).await?;
        }
        let audit = audit(directory.path());
        audit.execute_batch("CREATE TRIGGER refuse_ack BEFORE UPDATE OF ack_sequence,ack_ordinal ON task_outbox_consumers BEGIN SELECT RAISE(FAIL,'fixture ACK failure'); END;").unwrap();
        assert!(
            h.publisher
                .publish(PublishRequest { limit: 256 })
                .await
                .is_err()
        );
        assert!(acknowledgements(&audit).is_empty());
        let status = h.storage.outbox_status().await?;
        assert!(status.consumers[0].acknowledged.is_none());
        let events = h.sessions.events(EventQuery::new("chat")).await?.events;
        assert_eq!(
            events.len() as u64,
            status.retained_events,
            "the complete bounded page was appended before its cumulative ACK"
        );
        let before = requests(directory.path());
        let committed = h.sessions.committed().value()?;
        audit.execute_batch("DROP TRIGGER refuse_ack").unwrap();
        assert_eq!(
            h.publisher
                .publish(PublishRequest { limit: 1 })
                .await?
                .published,
            1
        );
        assert!(
            h.publisher
                .publish(PublishRequest { limit: 256 })
                .await?
                .published
                > 0
        );
        assert_eq!(acknowledgements(&audit).len(), 2);
        assert_eq!(requests(directory.path()), before);
        assert_eq!(h.sessions.committed().value()?, committed);
        assert_eq!(
            h.sessions.events(EventQuery::new("chat")).await?.events,
            events
        );
        assert_eq!(h.storage.outbox_status().await?.pending_events, 0);
        h.host.close().await
    })
}
