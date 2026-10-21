//! Joined Agent admission uses the real binding owner and immutable durable store identities.

use futures::FutureExt;
use serde_json::json;
use std::time::Duration;
use swarmie_agent_contracts::*;
use swarmie_agent_loop_fixture::{hooks, model::Reply};
use swarmie_session_contracts::{CreateSession, EventQuery};
use swarmie_task_contracts::{TaskLookup, TaskOutcome, TaskQuery, TaskStatus};
use swarmie_task_session_contracts::*;
use tembra::Result;
use tembra_hooks::HookArgs;

pub mod conversation_support;
pub mod support;
use conversation_support::*;
use support::*;

async fn binding(h: &Harness, task_id: &str) -> Result<TaskSessionBinding> {
    let disposition = h
        .bindings
        .lookup(BindingKey {
            task_store_id: h.storage.outbox_status().await?.store_id,
            task_id: task_id.into(),
        })
        .await?;
    let Some(BindingDisposition::Routed { binding }) = disposition else {
        panic!("expected retained routed intent")
    };
    Ok(*binding)
}

#[test]
fn concurrent_exact_start_retains_one_original_deadline_across_host_restart() {
    run(|driver| async move {
        let root = tempfile::tempdir().unwrap();
        let h = open(
            driver.clone(),
            root.path(),
            vec![Reply::text("once")],
            Default::default(),
            true,
        )
        .await?;
        seed(&h.sessions, "session", "conversation", "input", "hello").await?;
        let request = request("session", "conversation", "turn", "input");
        let (first, second) = futures::join!(
            h.agent.start(request.clone()),
            h.agent.start(request.clone())
        );
        let first = first?;
        assert_eq!(first.task.id, second?.task.id);
        settle(&h.tasks, &first).await?;
        let original = binding(&h, &first.task.id).await?;
        assert_eq!(original.admission, BindingAdmission::Active);
        assert_eq!(original.task.input, serde_json::to_value(&request).unwrap());
        assert_eq!(
            original.task.checkpoint["started_at_ms"],
            first.task.checkpoint["started_at_ms"]
        );
        assert_eq!(h.tasks.graph(TaskQuery::default()).await?.records.len(), 1);
        assert_eq!(h.model.requests.borrow().len(), 1);
        h.host.close().await?;
        tokio::time::sleep(Duration::from_millis(5)).await;
        let reopened = open(
            driver,
            root.path(),
            vec![Reply::text("must not resubmit")],
            Default::default(),
            true,
        )
        .await?;
        let retried = reopened.agent.start(request).await?;
        assert_eq!(retried.task.id, first.task.id);
        let retained = binding(&reopened, &first.task.id).await?;
        assert_eq!(retained.task, original.task);
        assert_eq!(retained.destination, original.destination);
        assert!(reopened.model.requests.borrow().is_empty());
        reopened.host.close().await
    });
}

#[test]
fn recovered_unbound_and_excluded_agent_tasks_fail_closed_without_effects() {
    run(|driver| async move {
        let template_root = tempfile::tempdir().unwrap();
        let template = open(
            driver.clone(),
            template_root.path(),
            vec![Reply::text("template")],
            Default::default(),
            true,
        )
        .await?;
        let initial = template.start("unbound", Default::default()).await?;
        template.terminal("unbound").await?;
        let spec = binding(&template, &initial.task.id).await?.task;
        template.host.close().await?;
        for excluded in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let h = open(
                driver.clone(),
                root.path(),
                vec![Reply::text("must not invoke")],
                Default::default(),
                true,
            )
            .await?;
            seed(
                &h.sessions,
                "session-unbound",
                "conversation",
                "input-unbound",
                "Compute the result.",
            )
            .await?;
            if excluded {
                h.bindings
                    .exclude(ExcludedBinding {
                        key: BindingKey {
                            task_store_id: h.storage.outbox_status().await?.store_id,
                            task_id: spec.id.clone(),
                        },
                        reason: "unrelated task".into(),
                    })
                    .await?;
            }
            h.tasks.create(spec.clone()).await?;
            loop {
                let saved = h
                    .tasks
                    .get(TaskLookup {
                        id: spec.id.clone(),
                    })
                    .await?
                    .unwrap();
                if saved.status == TaskStatus::Parked {
                    break;
                }
                assert!(
                    !saved.is_terminal(),
                    "unbound task must remain recoverable: {saved:?}"
                );
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
            assert!(h.model.requests.borrow().is_empty());
            assert!(h.tool.calls.borrow().is_empty());
            assert!(h.agent.inspect(Harness::lookup("unbound")).await.is_err());
            assert!(h.agent.cancel(Harness::lookup("unbound")).await.is_err());
            assert!(
                h.agent
                    .continue_turn(ContinueTurn {
                        session_id: "session-unbound".into(),
                        conversation_id: "conversation".into(),
                        turn_id: "unbound".into(),
                        resolution: None
                    })
                    .await
                    .is_err()
            );
            let saved = h
                .tasks
                .get(TaskLookup {
                    id: spec.id.clone(),
                })
                .await?
                .unwrap();
            assert_eq!(saved.input, spec.input);
            assert_eq!(saved.checkpoint, spec.checkpoint);
            assert!(!saved.cancellation_requested);
            assert_eq!(h.events("unbound").await?.len(), 1);
            h.host.close().await?;
        }
        Ok(())
    });
}

#[test]
fn joined_publisher_routes_authoritative_facts_once_without_repeating_effects() {
    run(|driver| async move {
        let root = tempfile::tempdir().unwrap();
        let h = open(
            driver,
            root.path(),
            vec![Reply::double(21), Reply::text("42")],
            Default::default(),
            true,
        )
        .await?;
        let started = h.start("published", Default::default()).await?;
        h.terminal("published").await?;
        let before = h.publisher().await?.diagnostics().await?;
        assert!(before.lag > 0);
        let mut acknowledged = 0;
        loop {
            let receipt = h
                .publisher()
                .await?
                .publish(PublishRequest { limit: 2 })
                .await?;
            acknowledged += receipt.published;
            if receipt.published + receipt.excluded == 0 {
                break;
            }
        }
        assert!(acknowledged > 2);
        assert_eq!(
            h.publisher()
                .await?
                .publish(PublishRequest { limit: 256 })
                .await?
                .published,
            0
        );
        let events = h.events("published").await?;
        let facts = events
            .iter()
            .filter(|event| event.event.kind == "swarmie.task-transition")
            .collect::<Vec<_>>();
        assert_eq!(facts.len(), acknowledged as usize);
        let mut ids = std::collections::BTreeSet::new();
        for fact in &facts {
            assert!(ids.insert(&fact.event.event_id));
            assert_eq!(
                fact.event.correlation.task_id.as_deref(),
                Some(started.task.id.as_str())
            );
            assert_eq!(
                fact.event.correlation.conversation_id.as_deref(),
                Some("conversation")
            );
            assert_eq!(fact.event.correlation.turn_id.as_deref(), Some("published"));
            assert_eq!(
                fact.event.payload["source"]["id"]["store_id"],
                h.binding_configuration.task_store_id
            );
            assert_eq!(fact.event.payload["origin"], "task_storage_outbox");
        }
        assert!(
            facts
                .windows(2)
                .all(|pair| pair[0].event.payload["source"]["revision"].as_u64()
                    < pair[1].event.payload["source"]["revision"].as_u64())
        );
        assert_eq!(h.model.requests.borrow().len(), 2);
        assert_eq!(h.tool.calls.borrow().len(), 1);
        assert_eq!(h.publisher().await?.diagnostics().await?.lag, 0);
        h.host.close().await
    });
}

fn pause_before_generation() -> Result<tembra::NativeFacet> {
    hooks::contribution(
        before_model_call()?,
        "freeze-before-generation",
        0,
        |_: HookArgs<ModelCall, ModelCall>, context| {
            async move { Err(context.cancellation().cancelled().await) }.boxed_local()
        },
    )
}

#[test]
fn replaced_destination_blocks_inspect_and_continue_but_allows_source_cancellation() {
    run(|driver| async move {
        let root = tempfile::tempdir().unwrap();
        let h = open_with_contributions(
            driver.clone(),
            root.path(),
            vec![Reply::text("must not invoke")],
            vec![pause_before_generation()?],
        )
        .await?;
        let started = h.start("destination", Default::default()).await?;
        let original = binding(&h, &started.task.id).await?;
        let configuration = h.binding_configuration.clone();
        h.host.close().await?;
        std::fs::rename(
            root.path().join("sessions.sqlite"),
            root.path().join("original-sessions.sqlite"),
        )
        .unwrap();
        let reopened = open_degraded_bindings(driver, root.path(), configuration).await?;
        reopened
            .sessions
            .create(CreateSession {
                session_id: "session-destination".into(),
                metadata: json!({}),
            })
            .await?;
        assert_ne!(
            reopened
                .sessions
                .publication_status()
                .await?
                .durable
                .store_id,
            original.destination.session_store_id
        );
        assert!(
            reopened
                .agent
                .inspect(Harness::lookup("destination"))
                .await
                .is_err()
        );
        assert!(
            reopened
                .agent
                .continue_turn(ContinueTurn {
                    session_id: "session-destination".into(),
                    conversation_id: "conversation".into(),
                    turn_id: "destination".into(),
                    resolution: None
                })
                .await
                .is_err()
        );
        let cancelled = reopened
            .agent
            .cancel(Harness::lookup("destination"))
            .await?;
        assert_eq!(cancelled.task.id, started.task.id);
        let saved = reopened
            .tasks
            .wait(TaskLookup {
                id: started.task.id,
            })
            .await?;
        assert!(matches!(saved, TaskOutcome::Aborted { .. }));
        assert!(reopened.model.requests.borrow().is_empty());
        assert!(reopened.tool.calls.borrow().is_empty());
        assert!(
            reopened
                .sessions
                .events(EventQuery::new("session-destination"))
                .await?
                .events
                .is_empty()
        );
        assert_eq!(
            binding(&reopened, &cancelled.task.id).await?.destination,
            original.destination
        );
        reopened.host.close().await
    });
}

#[test]
fn prepared_intent_reuses_original_checkpoint_when_start_reconciles_absent_task() {
    run(|driver| async move {
        let source = tempfile::tempdir().unwrap();
        let template = open(
            driver.clone(),
            source.path(),
            vec![Reply::text("template")],
            Default::default(),
            true,
        )
        .await?;
        let started = template.start("pending", Default::default()).await?;
        template.terminal("pending").await?;
        let retained = binding(&template, &started.task.id).await?.task;
        template.host.close().await?;
        tokio::time::sleep(Duration::from_millis(10)).await;
        let root = tempfile::tempdir().unwrap();
        let h = open(
            driver,
            root.path(),
            vec![Reply::text("admitted once")],
            Default::default(),
            true,
        )
        .await?;
        seed(
            &h.sessions,
            "session-pending",
            "conversation",
            "input-pending",
            "Compute the result.",
        )
        .await?;
        let pending = h
            .bindings
            .prepare(PrepareBinding {
                task_store_id: h.binding_configuration.task_store_id.clone(),
                task: retained.clone(),
                destination: SessionDestination {
                    session_store_id: h.binding_configuration.session_store_id.clone(),
                    session_id: "session-pending".into(),
                    conversation_id: "conversation".into(),
                    turn_id: "pending".into(),
                },
                routing_version: 1,
            })
            .await?;
        assert_eq!(pending.admission, BindingAdmission::Pending);
        assert!(
            h.tasks
                .get(TaskLookup {
                    id: retained.id.clone()
                })
                .await?
                .is_none()
        );
        let request: StartTurn = serde_json::from_value(retained.input.clone()).unwrap();
        let submitted = h.agent.start(request).await?;
        h.terminal("pending").await?;
        assert_eq!(
            submitted.task.checkpoint["started_at_ms"],
            retained.checkpoint["started_at_ms"]
        );
        assert_eq!(binding(&h, &retained.id).await?.task, retained);
        assert_eq!(h.model.requests.borrow().len(), 1);
        h.host.close().await
    });
}

#[test]
fn replaced_source_cannot_reuse_old_route_or_cancel_copied_task() {
    run(|driver| async move {
        let root = tempfile::tempdir().unwrap();
        let h = open(
            driver.clone(),
            root.path(),
            vec![Reply::text("original")],
            Default::default(),
            true,
        )
        .await?;
        let started = h.start("source", Default::default()).await?;
        h.terminal("source").await?;
        let original = binding(&h, &started.task.id).await?;
        let configuration = h.binding_configuration.clone();
        h.host.close().await?;
        std::fs::rename(
            root.path().join("tasks.sqlite"),
            root.path().join("original-tasks.sqlite"),
        )
        .unwrap();
        let reopened = open_degraded_bindings(driver, root.path(), configuration).await?;
        assert_ne!(
            reopened.storage.outbox_status().await?.store_id,
            original.key.task_store_id
        );
        reopened.tasks.create(original.task.clone()).await?;
        loop {
            let record = reopened
                .tasks
                .get(TaskLookup {
                    id: original.task.id.clone(),
                })
                .await?
                .unwrap();
            if record.status == TaskStatus::Parked {
                break;
            }
            assert!(!record.is_terminal());
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        assert!(
            reopened
                .agent
                .inspect(Harness::lookup("source"))
                .await
                .is_err()
        );
        assert!(
            reopened
                .agent
                .cancel(Harness::lookup("source"))
                .await
                .is_err()
        );
        assert!(reopened.model.requests.borrow().is_empty());
        assert!(reopened.tool.calls.borrow().is_empty());
        let Some(BindingDisposition::Routed { binding: retained }) =
            reopened.bindings.lookup(original.key.clone()).await?
        else {
            panic!("original historical route must survive")
        };
        assert_eq!(*retained, original);
        let copied = reopened
            .tasks
            .get(TaskLookup {
                id: original.task.id,
            })
            .await?
            .unwrap();
        assert!(!copied.cancellation_requested);
        reopened.host.close().await
    });
}
