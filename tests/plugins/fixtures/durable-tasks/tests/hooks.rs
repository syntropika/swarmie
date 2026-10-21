//! Actual SQLite and Tembra-managed observer acceptance; no callback-only scheduler mocks.

use std::{collections::BTreeSet, future::Future, rc::Rc, time::Duration};

use serde_json::{Value, json};
use swarmie_durable_tasks_fixture::{
    adversarial, concurrency, facet,
    hooks::{self, Observer},
    task,
};
use swarmie_task_contracts::*;
use tembra::{core, prelude::*};
use tembra_native::TokioDriver;

fn run<F, Fut>(test: F)
where
    F: FnOnce(Rc<TokioDriver>) -> Fut,
    Fut: Future<Output = Result<()>>,
{
    let driver = TokioDriver::new();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(driver.run_until(async {
            tokio::time::timeout(Duration::from_secs(20), test(driver.clone()))
                .await
                .expect("lifecycle hook acceptance timed out")
                .unwrap();
        }));
}

async fn open(
    driver: Rc<TokioDriver>,
    observers: Vec<(&str, i64, Rc<Observer>)>,
) -> Result<(Host, DurableTasksRef, tempfile::TempDir)> {
    open_with_control(driver, observers, Rc::new(adversarial::Control::default())).await
}

async fn open_with_control(
    driver: Rc<TokioDriver>,
    observers: Vec<(&str, i64, Rc<Observer>)>,
    control: Rc<adversarial::Control>,
) -> Result<(Host, DurableTasksRef, tempfile::TempDir)> {
    open_config(driver, observers, control, 1).await
}

async fn open_config(
    driver: Rc<TokioDriver>,
    observers: Vec<(&str, i64, Rc<Observer>)>,
    control: Rc<adversarial::Control>,
    version: u32,
) -> Result<(Host, DurableTasksRef, tempfile::TempDir)> {
    let directory = tempfile::tempdir().unwrap();
    let host = Host::with_driver("lifecycle-hooks", "application", driver.clone())?;
    host.install(
        swarmie_task_storage_sqlite::facet(&swarmie_task_storage_sqlite::Configuration {
            database: directory.path().join("tasks.sqlite"),
        })?,
        core::LoadOptions::default(),
    )?
    .await?;
    host.install(facet(version)?, core::LoadOptions::default())?
        .await?;
    host.install(
        adversarial::facet(control, 1)?,
        core::LoadOptions::default(),
    )?
    .await?;
    for (name, priority, observer) in observers {
        host.install(
            hooks::facet(name, priority, observer)?,
            core::LoadOptions::default(),
        )?
        .await?;
    }
    host.install(
        swarmie_durable_tasks::facet(
            &swarmie_durable_tasks::Configuration {
                poll_interval_ms: 2,
                ..Default::default()
            },
            driver,
        )?,
        core::LoadOptions::default(),
    )?
    .await?;
    let tasks = host.get(DurableTasks, DurableTasksInput {}).await?;
    Ok((host, tasks, directory))
}

async fn eventually(mut condition: impl FnMut() -> bool) {
    while !condition() {
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
}

async fn status(tasks: &DurableTasksRef, id: &str, expected: TaskStatus) -> Result<TaskRecord> {
    loop {
        let record = tasks
            .get(TaskLookup { id: id.into() })
            .await?
            .expect("persisted task");
        if record.status == expected {
            return Ok(record);
        }
        assert!(
            !record.is_terminal(),
            "expected {expected:?}, got {record:?}"
        );
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
}

fn events(observer: &Observer) -> Vec<Value> {
    observer
        .events
        .borrow()
        .iter()
        .map(|(_, event)| serde_json::to_value(event).unwrap())
        .collect()
}

fn observed(observer: &Observer, id: &str, current: &str) -> bool {
    events(observer)
        .iter()
        .any(|event| event["task"]["task_id"] == id && event["current_status"] == current)
}

async fn terminal(tasks: &DurableTasksRef, id: &str) -> Result<TaskOutcome> {
    tasks.wait(TaskLookup { id: id.into() }).await
}

#[test]
fn zero_handlers_preserves_task_results_and_retains_no_notifications() {
    run(|driver| async move {
        let (host, tasks, _directory) = open(driver, vec![]).await?;
        tasks.create(task("without-observers")).await?;
        assert!(matches!(
            terminal(&tasks, "without-observers").await?,
            TaskOutcome::Completed { .. }
        ));
        let observations = host.get(TaskLifecycle, TaskLifecycleInput {}).await?;
        let diagnostics = serde_json::to_value(observations.diagnostics().await?).unwrap();
        assert_eq!(diagnostics["queued"], 0);
        assert_eq!(diagnostics["active"], false);
        host.close().await
    });
}

#[test]
fn native_observer_sees_confirmed_checkpoints_and_one_terminal_per_revision() {
    run(|driver| async move {
        let observer = Rc::new(Observer::default());
        let (host, tasks, _directory) = open(driver, vec![("audit", 0, observer.clone())]).await?;
        tasks.create(task("checkpointed")).await?;
        terminal(&tasks, "checkpointed").await?;
        eventually(|| observed(&observer, "checkpointed", "terminal")).await;
        let before = events(&observer);
        let committed: Vec<_> = before
            .iter()
            .filter(|event| event["type"] == "committed")
            .collect();
        assert!(
            committed
                .iter()
                .any(|event| event["checkpoint_committed"] == true)
        );
        assert!(before.iter().any(|event| event["type"] == "phase_settled"));
        let revisions: Vec<_> = committed
            .iter()
            .map(|event| event["task"]["revision"].as_u64().unwrap())
            .collect();
        assert_eq!(
            revisions.len(),
            revisions.iter().collect::<BTreeSet<_>>().len()
        );
        for _ in 0..5 {
            tasks.resume().await?;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(
            before,
            events(&observer),
            "routine reconciliation must not replay notifications"
        );
        host.close().await
    });
}

#[test]
fn observer_priority_uses_tembra_series_order_for_every_event() {
    run(|driver| async move {
        let observer = Rc::new(Observer::default());
        let (host, tasks, _directory) = open(
            driver,
            vec![
                ("later", 10, observer.clone()),
                ("first", -10, observer.clone()),
            ],
        )
        .await?;
        let mut spec = task("priority");
        spec.phase = "finish".into();
        tasks.create(spec).await?;
        terminal(&tasks, "priority").await?;
        eventually(|| {
            observer.events.borrow().iter().any(|(name, event)| {
                name == "later"
                    && serde_json::to_value(event).unwrap()["current_status"] == "terminal"
            })
        })
        .await;
        {
            let observed = observer.events.borrow();
            assert_eq!(observed.len() % 2, 0);
            for pair in observed.as_chunks::<2>().0 {
                assert_eq!(pair[0].0, "first");
                assert_eq!(pair[1].0, "later");
                assert_eq!(pair[0].1, pair[1].1);
            }
        }
        host.close().await
    });
}

#[test]
fn failed_observer_cannot_invalidate_commits_and_failure_is_diagnosed() {
    run(|driver| async move {
        let observer = Rc::new(Observer::default());
        observer.fail.set(true);
        let (host, tasks, _directory) = open(driver, vec![("broken", 0, observer.clone())]).await?;
        let created = tasks.create(task("committed-despite-observer")).await?;
        assert_eq!(created.revision, 1);
        assert!(matches!(
            terminal(&tasks, &created.id).await?,
            TaskOutcome::Completed { .. }
        ));
        eventually(|| observed(&observer, &created.id, "terminal")).await;
        let diagnostics = host.get(TaskLifecycle, TaskLifecycleInput {}).await?;
        loop {
            let value = serde_json::to_value(diagnostics.diagnostics().await?).unwrap();
            if value["failed"].as_u64().unwrap_or(0) > 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        assert_eq!(
            tasks
                .get(TaskLookup { id: created.id })
                .await?
                .unwrap()
                .status,
            TaskStatus::Terminal
        );
        host.close().await
    });
}

#[test]
fn waiting_sleeping_parent_child_cancellation_are_confirmed_observations() {
    run(|driver| async move {
        let observer = Rc::new(Observer::default());
        let (host, tasks, _directory) =
            open(driver, vec![("tree-audit", 0, observer.clone())]).await?;
        let mut parent = task("tree");
        parent.phase = "spawn".into();
        parent.input = json!({"children_sleep":true});
        tasks.create(parent).await?;
        status(&tasks, "tree", TaskStatus::Waiting).await?;
        for id in ["tree:first", "tree:second"] {
            status(&tasks, id, TaskStatus::Sleeping).await?;
        }
        tasks.cancel(TaskLookup { id: "tree".into() }).await?;
        assert!(matches!(
            terminal(&tasks, "tree").await?,
            TaskOutcome::Aborted { .. }
        ));
        eventually(|| {
            ["tree", "tree:first", "tree:second"]
                .iter()
                .all(|id| observed(&observer, id, "terminal"))
        })
        .await;
        assert!(observed(&observer, "tree", "waiting"));
        assert!(observed(&observer, "tree:first", "sleeping"));
        for id in ["tree", "tree:first", "tree:second"] {
            assert!(
                events(&observer)
                    .iter()
                    .any(|event| event["task"]["task_id"] == id
                        && event["cancellation_requested"] == true)
            );
        }
        host.close().await
    });
}

#[test]
fn parked_handler_retry_emits_new_confirmed_revisions() {
    run(|driver| async move {
        let observer = Rc::new(Observer::default());
        let (host, tasks, _directory) =
            open(driver, vec![("retry-audit", 0, observer.clone())]).await?;
        tasks
            .create(TaskSpec {
                id: "parked".into(),
                kind: "fixture.adversarial".into(),
                definition_version: 1,
                input: Value::Null,
                checkpoint: Value::Null,
                phase: "exception".into(),
            })
            .await?;
        let first = status(&tasks, "parked", TaskStatus::Parked).await?;
        tasks
            .retry(TaskLookup {
                id: "parked".into(),
            })
            .await?;
        loop {
            let second = status(&tasks, "parked", TaskStatus::Parked).await?;
            if second.attempts > first.attempts {
                break;
            }
        }
        eventually(|| {
            events(&observer)
                .iter()
                .filter(|event| event["current_status"] == "parked")
                .count()
                == 2
        })
        .await;
        assert!(
            events(&observer)
                .iter()
                .any(|event| event["origin"] == "retry")
        );
        host.close().await
    });
}

#[test]
fn slow_observer_does_not_block_scheduler_and_host_joins_physical_settlement() {
    run(|driver| async move {
        let observer = Rc::new(Observer::default());
        observer.hold.set(true);
        let (host, tasks, _directory) =
            open(driver, vec![("stubborn", 0, observer.clone())]).await?;
        tasks.create(task("keeps-running")).await?;
        eventually(|| observer.started.get()).await;
        assert!(matches!(
            terminal(&tasks, "keeps-running").await?,
            TaskOutcome::Completed { .. }
        ));
        let close = host.close();
        tokio::pin!(close);
        tokio::select! {
            result = &mut close => panic!("Host closed before observer physically settled: {result:?}"),
            _ = tokio::time::sleep(Duration::from_millis(30)) => {},
        }
        assert!(observer.cancelled.get());
        observer.release();
        close.await
    });
}

#[test]
fn observation_can_reenter_cancel_and_resume_without_scheduler_lock_deadlock() {
    run(|driver| async move {
        let observer = Rc::new(Observer::default());
        let (host, tasks, _directory) =
            open(driver, vec![("reentrant", 0, observer.clone())]).await?;
        *observer.reentrant_cancel.borrow_mut() =
            Some((tasks.clone(), "cancel-from-observer".into()));
        let mut sleeper = task("cancel-from-observer");
        sleeper.phase = "sleep".into();
        tasks.create(sleeper).await?;
        assert!(matches!(
            terminal(&tasks, "cancel-from-observer").await?,
            TaskOutcome::Aborted { .. }
        ));
        eventually(|| observed(&observer, "cancel-from-observer", "terminal")).await;
        host.close().await
    });
}

#[test]
fn cas_conflict_after_effect_never_publishes_fictitious_completion() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let backend = Host::with_driver("hooks-cas-backend", "application", driver.clone())?;
        backend
            .install(
                swarmie_task_storage_sqlite::facet(&swarmie_task_storage_sqlite::Configuration {
                    database: directory.path().join("tasks.sqlite"),
                })?,
                core::LoadOptions::default(),
            )?
            .await?;
        let storage = backend.get(TaskStorage, TaskStorageInput {}).await?;
        let control = concurrency::StorageControl::new(storage);
        control.injected_revision_conflicts.set(1);
        let observer = Rc::new(Observer::default());
        let host = Host::with_driver("hooks-cas-scheduler", "application", driver.clone())?;
        host.install(
            concurrency::storage_facet(control.clone())?,
            core::LoadOptions::default(),
        )?
        .await?;
        host.install(
            concurrency::handler_facet(control.clone())?,
            core::LoadOptions::default(),
        )?
        .await?;
        host.install(
            hooks::facet("cas-audit", 0, observer.clone())?,
            core::LoadOptions::default(),
        )?
        .await?;
        host.install(
            swarmie_durable_tasks::facet(
                &swarmie_durable_tasks::Configuration {
                    poll_interval_ms: 2,
                    ..Default::default()
                },
                driver,
            )?,
            core::LoadOptions::default(),
        )?
        .await?;
        let tasks = host.get(DurableTasks, DurableTasksInput {}).await?;
        tasks
            .create(TaskSpec {
                id: "parent".into(),
                kind: "fixture.concurrent".into(),
                definition_version: 1,
                input: json!({}),
                checkpoint: json!({"operation_id":"parent:effect"}),
                phase: "effect".into(),
            })
            .await?;
        let parked = status(&tasks, "parent", TaskStatus::Parked).await?;
        eventually(|| observed(&observer, "parent", "parked")).await;
        assert_eq!(control.effects("parent"), 1);
        assert!(!observed(&observer, "parent", "terminal"));
        assert!(
            events(&observer)
                .iter()
                .any(|event| event["type"] == "phase_settled")
        );
        let committed: Vec<_> = events(&observer)
            .into_iter()
            .filter(|event| event["type"] == "committed")
            .collect();
        assert_eq!(
            committed.last().unwrap()["task"]["revision"],
            parked.revision
        );
        assert!(committed.iter().all(|event| event["outcome"].is_null()));
        host.close().await?;
        backend.close().await
    });
}

#[test]
fn cancellation_during_task_invocation_emits_settlement_only_after_physical_join() {
    run(|driver| async move {
        let observer = Rc::new(Observer::default());
        let control = Rc::new(adversarial::Control::default());
        let (host, tasks, _directory) = open_with_control(
            driver,
            vec![("physical-audit", 0, observer.clone())],
            control.clone(),
        )
        .await?;
        tasks
            .create(TaskSpec {
                id: "held-phase".into(),
                kind: "fixture.adversarial".into(),
                definition_version: 1,
                input: Value::Null,
                checkpoint: json!({"original":true}),
                phase: "gate".into(),
            })
            .await?;
        eventually(|| control.started.get()).await;
        tasks
            .cancel(TaskLookup {
                id: "held-phase".into(),
            })
            .await?;
        tokio::time::sleep(Duration::from_millis(25)).await;
        assert!(
            !events(&observer)
                .iter()
                .any(|event| event["type"] == "phase_settled")
        );
        assert!(!observed(&observer, "held-phase", "terminal"));
        control.release();
        assert!(matches!(
            terminal(&tasks, "held-phase").await?,
            TaskOutcome::Aborted { .. }
        ));
        eventually(|| observed(&observer, "held-phase", "terminal")).await;
        assert_eq!(
            events(&observer)
                .iter()
                .filter(|event| event["type"] == "phase_settled")
                .count(),
            1
        );
        host.close().await
    });
}

#[test]
fn timeout_and_backpressure_keep_one_physical_observer_and_bounded_pending_work() {
    run(|driver| async move {
        let observer = Rc::new(Observer::default());
        observer.hold.set(true);
        let (host, tasks, _directory) =
            open(driver, vec![("saturated", 0, observer.clone())]).await?;
        let diagnostics = host.get(TaskLifecycle, TaskLifecycleInput {}).await?;
        for index in 0..100 {
            let mut spec = task(&format!("pressure-{index}"));
            spec.phase = "finish".into();
            tasks.create(spec).await?;
        }
        for index in 0..100 {
            terminal(&tasks, &format!("pressure-{index}")).await?;
        }
        eventually(|| observer.cancelled.get()).await;
        let state = diagnostics.diagnostics().await?;
        assert_eq!(
            observer.events.borrow().len(),
            1,
            "logical timeout must not free physical concurrency"
        );
        assert!(state.active);
        assert_eq!(state.timed_out, 1);
        assert!(state.dropped > 0);
        assert!(state.queued <= 256);
        let close = host.close();
        tokio::pin!(close);
        tokio::select! {
            result = &mut close => panic!("Host closed with physical observer ownership: {result:?}"),
            _ = tokio::time::sleep(Duration::from_millis(20)) => {},
        }
        observer.release();
        close.await
    });
}

#[test]
fn migration_commit_and_rejected_migration_remain_distinct_from_physical_settlement() {
    run(|driver| async move {
        let observer = Rc::new(Observer::default());
        let (host, tasks, _directory) = open_config(
            driver,
            vec![("migration-audit", 0, observer.clone())],
            Rc::new(adversarial::Control::default()),
            2,
        )
        .await?;
        tasks.create(task("accepted-migration")).await?;
        assert!(matches!(
            terminal(&tasks, "accepted-migration").await?,
            TaskOutcome::Completed { .. }
        ));
        eventually(|| observed(&observer, "accepted-migration", "terminal")).await;
        let accepted = events(&observer);
        assert!(
            accepted
                .iter()
                .any(|event| event["task"]["task_id"] == "accepted-migration"
                    && event["invocation"] == "migrate"
                    && event["outcome"] == "returned")
        );
        assert!(
            accepted
                .iter()
                .any(|event| event["task"]["task_id"] == "accepted-migration"
                    && event["task"]["definition_version"] == 2
                    && event["checkpoint_committed"] == true)
        );
        let mut rejected = task("rejected-migration");
        rejected.input = json!({"reject_migration":true});
        rejected.checkpoint = json!({"preserved":"old-format"});
        tasks.create(rejected.clone()).await?;
        let blocked = status(&tasks, &rejected.id, TaskStatus::Blocked).await?;
        eventually(|| observed(&observer, &rejected.id, "blocked")).await;
        assert_eq!(blocked.definition_version, 1);
        assert_eq!(blocked.checkpoint, rejected.checkpoint);
        assert!(
            events(&observer)
                .iter()
                .any(|event| event["task"]["task_id"] == rejected.id
                    && event["blocking"] == "migration_rejected"
                    && event["checkpoint_committed"] == false)
        );
        let migration_events = events(&observer)
            .into_iter()
            .filter(|event| event["task"]["task_id"] == rejected.id)
            .count();
        for _ in 0..3 {
            tasks.resume().await?;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(
            events(&observer)
                .into_iter()
                .filter(|event| event["task"]["task_id"] == rejected.id)
                .count(),
            migration_events
        );
        host.close().await
    });
}
