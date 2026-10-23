mod support;

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use serde_json::json;
use swarmie_session_contracts::*;
use swarmie_task_contracts::*;
use swarmie_task_session_contracts::*;
use tembra::{core, prelude::*};

use support::*;

async fn caller(host: &Host, name: &str) -> Result<core::FacetScope> {
    let captured = Rc::new(RefCell::new(None));
    let opening = captured.clone();
    host.install(
        NativeFacet::new(name, vec![], move |scope, _| {
            *opening.borrow_mut() = Some(scope.clone());
            Ok(())
        })?,
        core::LoadOptions::default(),
    )?
    .await?;
    Ok(captured.borrow_mut().take().unwrap())
}

async fn register(
    scope: &core::FacetScope,
    storage: TaskStorageRef,
    id: &str,
) -> Result<TaskOutboxConsumerState> {
    let id = id.to_owned();
    scope
        .root(move |context| {
            Box::pin(async move {
                storage
                    .with_context(&Context::from_core(context))
                    .register_outbox_consumer(TaskOutboxConsumer { id })
                    .await
            })
        })
        .await
}

#[test]
fn capacity_repair_compaction_protects_all_consumers_and_rejects_wrong_owner() -> Result<()> {
    let directory = tempfile::tempdir().unwrap();
    run(|driver| async move {
        let limits = TaskOutboxLimits {
            max_events: 1,
            ..Default::default()
        };
        let h = Harness::open(directory.path(), driver, limits, false).await?;
        let owner = caller(&h.host, "fixture.retention-owner").await?;
        register(&owner, h.storage.clone(), "slow-consumer").await?;
        let intruder = caller(&h.host, "fixture.intruder").await?;
        let before = h.storage.outbox_status().await?;
        assert_eq!(
            register(&intruder, h.storage.clone(), &h.configuration.consumer_id)
                .await
                .unwrap_err()
                .reason,
            "task_storage_outbox_owner_mismatch"
        );
        assert_eq!(
            h.storage
                .register_outbox_consumer(TaskOutboxConsumer {
                    id: h.configuration.consumer_id.clone()
                })
                .await
                .unwrap_err()
                .reason,
            "task_storage_outbox_owner_required"
        );
        assert_eq!(h.storage.outbox_status().await?, before);
        let first = h.request("first", "chat", "one").await?;
        h.bindings.prepare(first.clone()).await?;
        h.bindings.admit(key(&first)).await?;
        let second = h.request("second", "chat", "two").await?;
        h.bindings.prepare(second.clone()).await?;
        assert_eq!(
            h.bindings.admit(key(&second)).await.unwrap_err().reason,
            "task_storage_outbox_full"
        );
        assert!(matches!(
            h.bindings.lookup(key(&second)).await?,
            Some(BindingDisposition::Routed { binding }) if binding.admission == BindingAdmission::Pending
        ));
        assert!(h.publisher.diagnostics().await?.capacity_exhausted);
        h.publisher.publish(PublishRequest { limit: 256 }).await?;
        assert_eq!(
            h.publisher
                .compact(TaskOutboxCompaction { limit: 4096 })
                .await?
                .removed,
            0
        );
        let storage = h.storage.clone();
        owner
            .root(move |context| {
                Box::pin(async move {
                    let storage = storage.with_context(&Context::from_core(context));
                    let page = storage
                        .read_outbox(TaskOutboxQuery {
                            consumer_id: "slow-consumer".into(),
                            after: None,
                            limit: 256,
                        })
                        .await?;
                    storage
                        .acknowledge_outbox(TaskOutboxAcknowledgement {
                            consumer_id: "slow-consumer".into(),
                            through: page.next.unwrap(),
                        })
                        .await
                })
            })
            .await?;
        assert_eq!(
            h.publisher
                .compact(TaskOutboxCompaction { limit: 4096 })
                .await?
                .removed,
            1
        );
        assert_eq!(
            h.bindings.admit(key(&second)).await?.admission,
            BindingAdmission::Active
        );
        assert_eq!(h.storage.outbox_status().await?.retained_events, 1);
        h.host.close().await
    })
}

#[test]
fn replacement_stores_cannot_adopt_existing_routes_or_consumer_history() -> Result<()> {
    for filename in ["tasks.sqlite", "sessions.sqlite"] {
        let directory = tempfile::tempdir().unwrap();
        run(|driver| async move {
            let h = Harness::open(
                directory.path(),
                driver.clone(),
                TaskOutboxLimits::default(),
                false,
            )
            .await?;
            let request = h.request("stable", "chat", "one").await?;
            h.bindings.prepare(request.clone()).await?;
            h.bindings.admit(key(&request)).await?;
            let pins = h.configuration.clone();
            h.host.close().await?;
            drop(h);
            std::fs::rename(
                directory.path().join(filename),
                directory.path().join(format!("original-{filename}")),
            )
            .unwrap();
            let error = Harness::open_pinned(
                directory.path(),
                driver.clone(),
                TaskOutboxLimits::default(),
                false,
                Some(pins),
            )
            .await
            .err()
            .unwrap();
            assert_eq!(error.reason, "binding_store_mismatch");
            // Recomputing configuration from the replacement cannot silently repin binding SQLite.
            let error = Harness::open(
                directory.path(),
                driver.clone(),
                TaskOutboxLimits::default(),
                false,
            )
            .await
            .err()
            .unwrap();
            assert_eq!(error.reason, "binding_store_mismatch");
            std::fs::rename(
                directory.path().join(filename),
                directory.path().join(format!("replacement-{filename}")),
            )
            .unwrap();
            std::fs::rename(
                directory.path().join(format!("original-{filename}")),
                directory.path().join(filename),
            )
            .unwrap();
            let h =
                Harness::open(directory.path(), driver, TaskOutboxLimits::default(), false).await?;
            assert_eq!(
                h.bindings
                    .require_confirmed_binding(key(&request))
                    .await?
                    .destination,
                request.destination
            );
            drain(&h.publisher).await?;
            h.host.close().await
        })?;
    }
    Ok(())
}

#[test]
fn managed_activity_reconciles_history_after_restart_without_manual_publish() -> Result<()> {
    let directory = tempfile::tempdir().unwrap();
    run(|driver| async move {
        let h = Harness::open(
            directory.path(),
            driver.clone(),
            TaskOutboxLimits::default(),
            false,
        )
        .await?;
        let request = h.request("automatic", "chat", "one").await?;
        h.bindings.prepare(request.clone()).await?;
        h.bindings.admit(key(&request)).await?;
        assert_eq!(
            h.sessions
                .open(SessionLookup {
                    session_id: "chat".into()
                })
                .await?
                .unwrap()
                .head,
            0
        );
        h.host.close().await?;
        drop(h);
        let h = Harness::open(directory.path(), driver, TaskOutboxLimits::default(), true).await?;
        loop {
            let state = h.storage.outbox_status().await?;
            if state.pending_events == 0 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert!(
            h.sessions
                .open(SessionLookup {
                    session_id: "chat".into()
                })
                .await?
                .unwrap()
                .head
                > 0
        );
        let owner = h.storage.outbox_status().await?.consumers;
        assert_eq!(
            owner[0].owner_facet,
            swarmie_task_session_integration::FACET_ID
        );
        h.host.close().await
    })
}

#[test]
fn pending_intent_exact_retry_conflict_and_confirmed_effect_gate() -> Result<()> {
    let directory = tempfile::tempdir().unwrap();
    run(|driver| async move {
        let h = Harness::open(directory.path(), driver, TaskOutboxLimits::default(), false).await?;
        let request = h.request("root", "chat", "conversation").await?;
        let key = key(&request);
        assert_eq!(
            h.bindings
                .require_confirmed_binding(key.clone())
                .await
                .unwrap_err()
                .reason,
            "binding_missing"
        );
        let pending = h.bindings.prepare(request.clone()).await?;
        assert_eq!(pending.admission, BindingAdmission::Pending);
        assert!(
            h.tasks
                .get(TaskLookup { id: "root".into() })
                .await?
                .is_none()
        );
        assert_eq!(
            h.bindings.lookup(key.clone()).await?,
            Some(BindingDisposition::Routed {
                binding: Box::new(pending.clone())
            })
        );
        assert_eq!(
            h.bindings
                .require_confirmed_binding(key.clone())
                .await
                .unwrap_err()
                .reason,
            "binding_task_missing"
        );
        let mut changed = request.clone();
        changed.destination.conversation_id = "other".into();
        assert_eq!(
            h.bindings.prepare(changed).await.unwrap_err().reason,
            "binding_conflict"
        );
        assert_eq!(h.bindings.prepare(request.clone()).await?, pending);
        let active = h.bindings.admit(key.clone()).await?;
        assert_eq!(active.admission, BindingAdmission::Active);
        assert_eq!(h.bindings.admit(key.clone()).await?, active);
        assert_eq!(h.bindings.prepare(request).await?, active);
        assert_eq!(
            h.bindings
                .exclude(ExcludedBinding {
                    key: key.clone(),
                    reason: "unrelated".into()
                })
                .await
                .unwrap_err()
                .reason,
            "binding_conflict"
        );
        assert_eq!(h.bindings.require_confirmed_binding(key).await?, active);
        drain(&h.publisher).await?;
        h.host.close().await
    })
}

#[test]
fn missing_mapping_stops_contiguous_prefix_and_explicit_repair_preserves_destinations() -> Result<()>
{
    let directory = tempfile::tempdir().unwrap();
    run(|driver| async move {
        let h = Harness::open(directory.path(), driver, TaskOutboxLimits::default(), false).await?;
        for (id, session, conversation) in [
            ("a", "chat", "one"),
            ("b", "chat", "two"),
            ("c", "other", "one"),
        ] {
            let request = h.request(id, session, conversation).await?;
            h.bindings.prepare(request.clone()).await?;
            h.bindings.admit(key(&request)).await?;
        }
        h.tasks.create(spec("unrelated")).await?;
        let late = h.request("late", "chat", "two").await?;
        h.bindings.prepare(late.clone()).await?;
        h.bindings.admit(key(&late)).await?;
        assert_eq!(
            h.publisher
                .publish(PublishRequest { limit: 256 })
                .await
                .unwrap_err()
                .reason,
            "binding_missing"
        );
        let diagnostics = h.publisher.diagnostics().await?;
        assert_eq!(
            diagnostics.missing_binding.as_ref().unwrap().task_id,
            "unrelated"
        );
        assert!(diagnostics.lag > 0);
        let before = h.sessions.events(EventQuery::new("chat")).await?;
        assert!(
            !before
                .events
                .iter()
                .any(|e| e.event.correlation.task_id.as_deref() == Some("late"))
        );
        let unrelated = BindingKey {
            task_store_id: h.configuration.task_store_id.clone(),
            task_id: "unrelated".into(),
        };
        assert_eq!(
            h.bindings
                .prepare(h.request("unrelated", "chat", "one").await?)
                .await
                .unwrap_err()
                .reason,
            "binding_admission_order"
        );
        h.bindings
            .exclude(ExcludedBinding {
                key: unrelated.clone(),
                reason: "explicit unrelated fixture policy".into(),
            })
            .await?;
        assert_eq!(
            h.bindings
                .require_confirmed_binding(unrelated)
                .await
                .unwrap_err()
                .reason,
            "binding_excluded"
        );
        drain(&h.publisher).await?;
        let once = h.sessions.events(EventQuery::new("chat")).await?;
        assert!(
            once.events
                .iter()
                .any(|e| e.event.correlation.task_id.as_deref() == Some("late"))
        );
        for event in &once.events {
            let id = event.event.correlation.task_id.as_deref().unwrap();
            assert_eq!(
                event.event.correlation.conversation_id.as_deref(),
                Some(if id == "a" { "one" } else { "two" })
            );
            assert_eq!(
                event.event.payload["source"]["id"]["store_id"],
                h.configuration.task_store_id
            );
        }
        drain(&h.publisher).await?;
        assert_eq!(
            h.sessions.events(EventQuery::new("chat")).await?.events,
            once.events
        );
        let other = h.sessions.events(EventQuery::new("other")).await?;
        assert!(
            other
                .events
                .iter()
                .all(|e| e.event.correlation.task_id.as_deref() == Some("c"))
        );
        h.host.close().await
    })
}

#[tembra::contract(local, name = "fixture.bridge-handler", version = 1)]
trait Handler {
    #[portable]
    async fn step(&self, invocation: TaskInvocation) -> Result<TaskDecision>;
}

struct Workflow {
    bindings: TaskSessionBindingsRef,
    storage: TaskStorageRef,
    effects: Rc<Cell<u32>>,
}

impl HandlerProvider for Workflow {
    async fn step(&self, invocation: TaskInvocation, context: Context) -> Result<TaskDecision> {
        self.transition(invocation, context)
            .await
            .map(TaskDecision::from)
    }
}

impl Workflow {
    async fn transition(
        &self,
        invocation: TaskInvocation,
        context: Context,
    ) -> Result<TaskTransition> {
        let TaskInvocation::Run { task, .. } = invocation else {
            return Ok(TaskTransition::RejectMigration {
                reason: "unsupported".into(),
            });
        };
        let store = self
            .storage
            .with_context(&context)
            .identity()
            .await?
            .store_id;
        let key = BindingKey {
            task_store_id: store,
            task_id: task.id.clone(),
        };
        // The actual effect boundary closes the pending-intent/create-return race.
        self.bindings
            .with_context(&context)
            .require_confirmed_binding(key.clone())
            .await?;
        if task.id == "parent" && task.phase == "start" {
            let mut children = vec![];
            for (id, conversation) in [("child", None), ("fork", Some("forked".into()))] {
                let task = spec(id);
                self.bindings
                    .with_context(&context)
                    .prepare_child(PrepareChildBinding {
                        parent: key.clone(),
                        task: task.clone(),
                        conversation_id: conversation,
                    })
                    .await?;
                children.push(task);
            }
            return Ok(TaskTransition::Wait {
                checkpoint: json!({}),
                phase: "finish".into(),
                children,
                task_ids: vec![],
                policy: WaitPolicy::AllSettled,
            });
        }
        self.effects.set(self.effects.get() + 1);
        Ok(TaskTransition::Complete {
            value: json!("confirmed effect"),
        })
    }
}

async fn handler(host: &Host, effects: Rc<Cell<u32>>) -> Result<()> {
    host.install(
        NativeFacet::new("fixture.bridge-handler", vec![], move |_, setup| {
            let effects = effects.clone();
            setup.serve_with_options(
                Handler,
                move |_, _, declaration| {
                    declaration.core_mut().register_method(
                        &task_definitions()?,
                        "fixture.bridge",
                        json!({"kind":"fixture.bridge","version":1}),
                        "step",
                    )?;
                    Ok(Workflow {
                        bindings: declaration
                            .get(TaskSessionBindings, TaskSessionBindingsInput {})?,
                        storage: declaration.get(TaskStorage, TaskStorageInput {})?,
                        effects: effects.clone(),
                    })
                },
                core::ServeOptions {
                    lifetime: core::Lifetime::Host,
                    ..Default::default()
                },
            )
        })?,
        core::LoadOptions::default(),
    )?
    .await
}

#[test]
fn managed_handlers_confirm_pending_and_children_inherit_before_wait_commit() -> Result<()> {
    let directory = tempfile::tempdir().unwrap();
    run(|driver| async move {
        let h = Harness::open(directory.path(), driver, TaskOutboxLimits::default(), false).await?;
        let effects = Rc::new(Cell::new(0));
        handler(&h.host, effects.clone()).await?;
        let request = h.request("parent", "chat", "inherited").await?;
        h.bindings.prepare(request.clone()).await?;
        // Exercise scheduler entry without admit's post-create activation.
        h.tasks.create(request.task.clone()).await?;
        let outcome = h
            .tasks
            .wait(TaskLookup {
                id: "parent".into(),
            })
            .await?;
        assert!(matches!(outcome, TaskOutcome::Completed { .. }));
        assert_eq!(effects.get(), 3);
        for (id, conversation) in [("child", "inherited"), ("fork", "forked")] {
            let binding = h
                .bindings
                .require_confirmed_binding(BindingKey {
                    task_store_id: request.task_store_id.clone(),
                    task_id: id.into(),
                })
                .await?;
            assert_eq!(binding.root_task_id, "parent");
            assert_eq!(binding.parent_task_id.as_deref(), Some("parent"));
            assert_eq!(binding.destination.conversation_id, conversation);
        }
        drain(&h.publisher).await?;
        let events = h.sessions.events(EventQuery::new("chat")).await?;
        assert!(
            events
                .events
                .iter()
                .any(|e| e.event.correlation.conversation_id.as_deref() == Some("forked"))
        );
        h.host.close().await
    })
}

#[test]
fn unbound_handler_never_performs_an_effect() -> Result<()> {
    let directory = tempfile::tempdir().unwrap();
    run(|driver| async move {
        let h = Harness::open(directory.path(), driver, TaskOutboxLimits::default(), false).await?;
        let effects = Rc::new(Cell::new(0));
        handler(&h.host, effects.clone()).await?;
        h.tasks.create(spec("unbound")).await?;
        for _ in 0..40 {
            h.tasks.resume().await?;
            if h.tasks
                .get(TaskLookup {
                    id: "unbound".into(),
                })
                .await?
                .unwrap()
                .status
                == TaskStatus::Parked
            {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert_eq!(effects.get(), 0);
        assert_eq!(
            h.tasks
                .get(TaskLookup {
                    id: "unbound".into()
                })
                .await?
                .unwrap()
                .status,
            TaskStatus::Parked
        );
        h.host.close().await
    })
}
