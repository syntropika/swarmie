//! Real loop admission must not turn foreign context pages into confirmed model requests.

use serde_json::json;
use swarmie_session_contracts::*;
use swarmie_task_contracts::TaskOutcome;
use tembra::{core, prelude::*};

pub mod context_identity_support;
pub mod support;
use context_identity_support::{Fault, Probe};
use support::*;

fn check(fault: Fault) {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let backend = Host::with_driver("context-backend", "application", driver.clone())?;
        backend
            .install(
                swarmie_session_store_sqlite::facet(
                    &swarmie_session_store_sqlite::Configuration {
                        database: directory.path().join("context.sqlite"),
                    },
                    driver.clone(),
                )?,
                core::LoadOptions::default(),
            )?
            .await?;
        let source = backend.get(SessionStore, SessionStoreInput {}).await?;
        let probe = Probe::new(fault);
        let initial = source.publication_status().await?.durable;
        let fixture = open_with_session_provider(
            driver,
            directory.path(),
            context_identity_support::facet(source.clone(), initial, probe.clone())?,
        )
        .await?;
        *probe.storage.borrow_mut() = Some(fixture.storage.clone());
        source
            .create(CreateSession {
                session_id: "session-identity".into(),
                metadata: json!({}),
            })
            .await?;
        for start in [0, 128] {
            let end = (start + 128).min(129);
            source.append(AppendEvents {
                session_id: "session-identity".into(), batch_id: format!("history-{start}"), expected_head: None,
                events: (start..end).map(|index| NewEvent {
                    event_id: format!("history-{index}"), kind: MESSAGE.into(), version: 1,
                    correlation: Correlation { conversation_id: Some("conversation".into()), ..Default::default() },
                    occurred_at_ms: None, payload: json!({"role":"user","content":"verified history","reply_to":null,"tool_call_id":null}),
                }).collect(),
            }).await?;
        }
        fixture.start("identity", Default::default()).await?;
        let terminal = fixture.terminal("identity").await?;
        let events = fixture.events("identity").await?;
        if fault == Fault::None {
            assert!(matches!(
                terminal.task.outcome,
                Some(TaskOutcome::Completed { .. })
            ));
            assert_eq!(fixture.model.requests.borrow().len(), 1);
            assert!(
                events
                    .iter()
                    .any(|event| event.event.kind == "swarmie.agent.model-request")
            );
        } else {
            assert!(
                probe.injected.get(),
                "the test must reach context-only fault injection"
            );
            assert!(
                fixture.model.requests.borrow().is_empty(),
                "foreign context must not reach generation"
            );
            assert!(matches!(
                terminal.task.outcome,
                Some(TaskOutcome::Failed { .. })
            ));
            assert!(
                !events
                    .iter()
                    .any(|event| event.event.kind == "swarmie.agent.model-request"),
                "no foreign context may become a confirmed request"
            );
        }
        fixture.host.close().await?;
        backend.close().await
    });
}

#[test]
fn context_rejects_foreign_open_store() {
    check(Fault::OpenStore);
}

#[test]
fn context_rejects_foreign_open_session() {
    check(Fault::OpenSession);
}

#[test]
fn context_rejects_foreign_store_on_later_page() {
    check(Fault::PageStore);
}

#[test]
fn context_rejects_foreign_session_on_later_page() {
    check(Fault::PageSession);
}

#[test]
fn context_accepts_the_confirmed_destination_across_pages() {
    check(Fault::None);
}
