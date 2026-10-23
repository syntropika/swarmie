mod support;

use serde_json::{Value, json};
use support::*;
use swarmie_session_contracts::*;
use tembra::prelude::*;

fn nested(depth: usize, object: bool) -> Value {
    (0..depth).fold(Value::Null, |value, _| {
        if object {
            json!({"value": value})
        } else {
            json!([value])
        }
    })
}

#[test]
fn every_admitted_event_crosses_the_reference_in_pages_and_projections() -> Result<()> {
    let directory = tempfile::tempdir().unwrap();
    run(|host, driver| async move {
        let store = open(&host, &directory.path().join("sessions.sqlite"), driver).await?;
        create(&store, "chat").await?;
        for depth in [124, 125] {
            let request = batch(
                "chat",
                format!("depth:{depth}"),
                vec![event(
                    format!("event:{depth}"),
                    "plugin.deep",
                    nested(depth, false),
                )],
            );
            let before = store.committed().value()?;
            match store.append(request.clone()).await {
                Ok(receipt) => {
                    let page = store.events(EventQuery::new("chat")).await?;
                    assert_eq!(page.events.last().unwrap().event, request.events[0]);
                    let projected = store
                        .project(ProjectionQuery {
                            source: EventQuery::new("chat"),
                            kind: ProjectionKind::Trajectory,
                        })
                        .await?;
                    assert_eq!(projected.entries, page.events);
                    assert_eq!(store.append(request).await?, receipt);
                }
                Err(error) => {
                    assert_eq!(depth, 125, "the deepest readable event must be admitted");
                    assert_eq!(error.reason, "session_store_invalid");
                    assert_eq!(store.committed().value()?, before);
                }
            }
        }
        host.close().await
    })
}

#[test]
fn deepest_admitted_values_survive_retry_all_projections_and_restart() -> Result<()> {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("sessions.sqlite");
    let database = &database;
    for restart in [false, true] {
        run(|host, driver| async move {
            let store = open(&host, database, driver).await?;
            for object in [false, true] {
                let id = if object { "objects" } else { "arrays" };
                let create = CreateSession {
                    session_id: id.into(),
                    metadata: nested(127, object),
                };
                let record = store.create(create.clone()).await?;
                assert_eq!(record.metadata, create.metadata);
                assert_eq!(store.create(create).await?, record);
                assert_eq!(
                    store
                        .open(SessionLookup {
                            session_id: id.into()
                        })
                        .await?,
                    Some(record)
                );
                let request = batch(
                    id,
                    "deep",
                    vec![
                        event("opaque", "plugin.deep", nested(124, object)),
                        event(
                            "message",
                            MESSAGE,
                            json!({
                                "role": "user",
                                "content": nested(123, object),
                                "reply_to": null,
                                "tool_call_id": null,
                            }),
                        ),
                    ],
                );
                let before = store.committed().value()?;
                let receipt = store.append(request.clone()).await?;
                if restart {
                    assert_eq!(store.committed().value()?, before);
                }
                assert_eq!(store.append(request.clone()).await?, receipt);
                let page = store.events(EventQuery::new(id)).await?;
                assert_eq!(page.events.len(), 2);
                assert_eq!(page.events[0].event, request.events[0]);
                assert_eq!(page.events[1].event, request.events[1]);
                for kind in [
                    ProjectionKind::Trajectory,
                    ProjectionKind::Conversation,
                    ProjectionKind::ModelContext,
                ] {
                    let projected = store
                        .project(ProjectionQuery {
                            source: EventQuery::new(id),
                            kind,
                        })
                        .await?;
                    let expected = if kind == ProjectionKind::Trajectory {
                        &page.events[..]
                    } else {
                        &page.events[1..]
                    };
                    assert_eq!(projected.entries.as_slice(), expected);
                }
            }
            host.close().await
        })?;
    }
    Ok(())
}

#[test]
fn page_depth_refusal_leaves_no_events_receipt_or_watermark() -> Result<()> {
    let directory = tempfile::tempdir().unwrap();
    run(|host, driver| async move {
        let store = open(&host, &directory.path().join("sessions.sqlite"), driver).await?;
        create(&store, "chat").await?;
        let mut request = batch(
            "chat",
            "depth",
            vec![
                message("safe", "user", "Hello"),
                event("deep", "plugin.deep", nested(125, false)),
            ],
        );
        // The request itself crosses Tembra; admission must also account for result pages.
        assert!(canonical(&request).is_ok());
        let before = store.committed().value()?;
        let page = store.events(EventQuery::new("chat")).await?;
        assert_eq!(
            store.append(request.clone()).await.unwrap_err().reason,
            "session_store_invalid"
        );
        assert_eq!(store.committed().value()?, before);
        assert_eq!(store.events(EventQuery::new("chat")).await?, page);
        assert_eq!(
            store
                .open(SessionLookup {
                    session_id: "chat".into()
                })
                .await?
                .unwrap()
                .head,
            0
        );
        request.events[1].payload = nested(124, false);
        let receipt = store.append(request.clone()).await?;
        assert_eq!(receipt.first_sequence, 1);
        assert_eq!(receipt.last_sequence, 2);
        assert_eq!(receipt.commit_sequence, before.commit_sequence + 1);
        assert_eq!(store.append(request).await?, receipt);
        host.close().await
    })
}
