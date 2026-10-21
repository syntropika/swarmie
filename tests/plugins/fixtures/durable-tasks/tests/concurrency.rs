//! Real SQLite and managed operation races must not replay settled external phases.

use std::{future::Future, rc::Rc, time::Duration};

use serde_json::json;
use swarmie_durable_tasks_fixture::concurrency::{StorageControl, handler_facet, storage_facet};
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
            tokio::time::timeout(Duration::from_secs(30), test(driver.clone()))
                .await
                .expect("concurrency acceptance timed out")
                .unwrap();
        }));
}

struct Rig {
    host: Host,
    backend: Host,
    tasks: DurableTasksRef,
    control: Rc<StorageControl>,
    _directory: tempfile::TempDir,
}

async fn open(
    driver: Rc<TokioDriver>,
    records: Vec<TaskRecord>,
    configure: impl FnOnce(&StorageControl),
) -> Result<Rig> {
    let directory = tempfile::tempdir().unwrap();
    let backend = Host::with_driver("concurrency-backend", "application", driver.clone())?;
    backend
        .install(
            swarmie_task_storage_sqlite::facet(&swarmie_task_storage_sqlite::Configuration {
                database: directory.path().join("tasks.sqlite"),
                outbox: Default::default(),
            })?,
            core::LoadOptions::default(),
        )?
        .await?;
    let storage = backend.get(TaskStorage, TaskStorageInput {}).await?;
    if !records.is_empty() {
        storage
            .claim_owner(StorageOwner { id: "seed".into() })
            .await?;
        for chunk in records.chunks(1) {
            assert!(matches!(
                storage
                    .commit(TaskTransaction {
                        owner: "seed".into(),
                        writes: chunk
                            .iter()
                            .cloned()
                            .map(|record| TaskWrite {
                                expected_revision: None,
                                record
                            })
                            .collect()
                    })
                    .await?,
                CommitResult::Committed { .. }
            ));
        }
        storage
            .release_owner(StorageOwner { id: "seed".into() })
            .await?;
    }
    let control = StorageControl::new(storage);
    configure(&control);
    let host = Host::with_driver("concurrency-scheduler", "application", driver.clone())?;
    host.install(
        storage_facet(control.clone())?,
        core::LoadOptions::default(),
    )?
    .await?;
    host.install(
        handler_facet(control.clone())?,
        core::LoadOptions::default(),
    )?
    .await?;
    host.install(
        swarmie_durable_tasks::facet(
            &swarmie_durable_tasks::Configuration {
                poll_interval_ms: 2,
                max_parallel: 1,
                ..Default::default()
            },
            driver,
        )?,
        core::LoadOptions::default(),
    )?
    .await?;
    let tasks = host.get(DurableTasks, DurableTasksInput {}).await?;
    Ok(Rig {
        host,
        backend,
        tasks,
        control,
        _directory: directory,
    })
}

fn spec(id: &str) -> TaskSpec {
    TaskSpec {
        id: id.into(),
        kind: "fixture.concurrent".into(),
        definition_version: 1,
        input: json!({}),
        checkpoint: json!({"operation_id":format!("{id}:effect")}),
        phase: "effect".into(),
    }
}

async fn status(tasks: &DurableTasksRef, id: &str, expected: TaskStatus) -> Result<TaskRecord> {
    loop {
        let record = tasks.get(TaskLookup { id: id.into() }).await?.unwrap();
        if record.status == expected {
            return Ok(record);
        }
        if record.is_terminal() {
            return Err(RuntimeError::invalid(
                "fixture_unexpected_terminal",
                format!("expected {expected:?}, got {record:?}"),
            ));
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
}

#[test]
fn existing_child_identity_parks_once_without_replaying_an_external_phase() {
    run(|driver| async move {
        let rig = open(driver, vec![], |_| {}).await?;
        let mut existing = spec("collision");
        existing.kind = "fixture.missing".into();
        rig.tasks.create(existing).await?;
        let mut parent = spec("parent");
        parent.input = json!({"collision":true});
        let checkpoint = parent.checkpoint.clone();
        rig.tasks.create(parent).await?;
        let parked = status(&rig.tasks, "parent", TaskStatus::Parked).await?;
        assert!(
            matches!(
                parked.blocking_reason,
                Some(BlockingReason::HandlerFailure { .. })
            ),
            "{parked:?}"
        );
        assert_eq!(parked.checkpoint, checkpoint);
        assert_eq!(rig.control.effects("parent"), 1);
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert_eq!(
            rig.control.effects("parent"),
            1,
            "a known atomic conflict must not replay the external phase"
        );
        let original = rig
            .tasks
            .get(TaskLookup {
                id: "collision".into(),
            })
            .await?
            .unwrap();
        assert!(original.parent.is_none());
        rig.host.close().await?;
        rig.backend.close().await
    });
}

#[test]
fn a_settled_phase_revision_conflict_requires_explicit_retry() {
    run(|driver| async move {
        let rig = open(driver, vec![], |control| {
            control.injected_revision_conflicts.set(1)
        })
        .await?;
        let parent = spec("parent");
        let checkpoint = parent.checkpoint.clone();
        rig.tasks.create(parent).await?;
        let parked = status(&rig.tasks, "parent", TaskStatus::Parked).await?;
        assert_eq!(parked.checkpoint, checkpoint);
        assert_eq!(rig.control.effects("parent"), 1);
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert_eq!(rig.control.effects("parent"), 1);
        rig.tasks
            .retry(TaskLookup {
                id: "parent".into(),
            })
            .await?;
        assert!(matches!(
            rig.tasks
                .wait(TaskLookup {
                    id: "parent".into()
                })
                .await?,
            TaskOutcome::Completed { .. }
        ));
        assert_eq!(rig.control.effects("parent"), 2);
        rig.host.close().await?;
        rig.backend.close().await
    });
}

#[test]
fn concurrent_public_cancellation_wins_over_a_settled_phase_commit() {
    run(|driver| async move {
        let rig = open(driver, vec![], |control| {
            control.pause_final_commit.set(true)
        })
        .await?;
        let parent = spec("parent");
        let checkpoint = parent.checkpoint.clone();
        rig.tasks.create(parent).await?;
        while !rig.control.final_commit_started.get() {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        let intent = rig
            .tasks
            .cancel(TaskLookup {
                id: "parent".into(),
            })
            .await?;
        assert!(intent.cancellation_requested);
        rig.control.release_commit();
        assert!(matches!(
            rig.tasks
                .wait(TaskLookup {
                    id: "parent".into()
                })
                .await?,
            TaskOutcome::Aborted { .. }
        ));
        assert_eq!(
            rig.tasks
                .get(TaskLookup {
                    id: "parent".into()
                })
                .await?
                .unwrap()
                .checkpoint,
            checkpoint
        );
        assert_eq!(rig.control.effects("parent"), 1);
        rig.host.close().await?;
        rig.backend.close().await
    });
}

#[test]
fn more_than_eight_real_cursor_invalidations_do_not_stop_the_scheduler() {
    run(|driver| async move {
        let mut records = (0..3)
            .map(|index| {
                let mut task = spec(&format!("task-{index:04}"));
                task.kind = "fixture.missing".into();
                TaskRecord::new(task, None)
            })
            .collect::<Vec<_>>();
        let mut churn = TaskRecord::new(spec("churn"), None);
        churn.status = TaskStatus::Sleeping;
        churn.wake_at_ms = Some(9_000_000_000_000_000);
        records.push(churn);
        let rig = open(driver, records, |control| {
            control.cursor_invalidations.set(12);
            control.page_limit.set(1);
        })
        .await?;
        while rig.control.cursor_invalidations.get() != 0 {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        rig.tasks.create(spec("after-contention")).await?;
        assert!(matches!(
            rig.tasks
                .wait(TaskLookup {
                    id: "after-contention".into()
                })
                .await?,
            TaskOutcome::Completed { .. }
        ));
        assert_eq!(rig.control.effects("after-contention"), 1);
        rig.host.close().await?;
        rig.backend.close().await
    });
}

#[test]
fn settled_decision_never_overwrites_a_newer_checkpoint() {
    run(|driver| async move {
        let rig = open(driver, vec![], |control| {
            control.injected_revision_conflicts.set(1);
            control.injected_checkpoint_change.set(true);
        })
        .await?;
        rig.tasks.create(spec("parent")).await?;
        let parked = status(&rig.tasks, "parent", TaskStatus::Parked).await?;
        assert_eq!(parked.checkpoint, json!({"newer_checkpoint":true}));
        assert_eq!(parked.phase, "newer-phase");
        assert!(parked.wake_at_ms.is_none());
        assert!(parked.wait_for.is_empty());
        assert_eq!(rig.control.effects("parent"), 1);
        rig.host.close().await?;
        rig.backend.close().await
    });
}

#[test]
fn repeated_metadata_conflicts_retain_the_settled_phase_until_parking_commits() {
    run(|driver| async move {
        let rig = open(driver, vec![], |control| {
            control.injected_revision_conflicts.set(1);
            control.metadata_conflicts.set(12);
        })
        .await?;
        rig.tasks.create(spec("parent")).await?;
        status(&rig.tasks, "parent", TaskStatus::Parked).await?;
        assert_eq!(rig.control.metadata_conflicts.get(), 0);
        assert_eq!(rig.control.effects("parent"), 1);
        rig.host.close().await?;
        rig.backend.close().await
    });
}

#[test]
fn aggregate_large_children_fault_only_the_parent_before_handler_admission() {
    run(|driver| async move {
        let parent = TaskRecord::new(spec("large-parent"), None);
        let mut records = vec![parent];
        for id in ["large-child-a", "large-child-b"] {
            let mut child = TaskRecord::new(spec(id), Some("large-parent".into()));
            child.input = json!({"data":"x".repeat(600_000)});
            child.status = TaskStatus::Terminal;
            child.outcome = Some(TaskOutcome::Completed { value: json!(null) });
            records.push(child);
        }
        let rig = open(driver, records, |_| {}).await?;
        let parent = status(&rig.tasks, "large-parent", TaskStatus::Terminal).await?;
        assert!(matches!(parent.outcome, Some(TaskOutcome::Faulted { .. })));
        assert_eq!(rig.control.effects("large-parent"), 0);
        rig.tasks.create(spec("small-after-large")).await?;
        assert!(matches!(
            rig.tasks
                .wait(TaskLookup {
                    id: "small-after-large".into()
                })
                .await?,
            TaskOutcome::Completed { .. }
        ));
        rig.host.close().await?;
        rig.backend.close().await
    });
}

#[test]
fn stale_handler_errors_and_protocol_faults_preserve_newer_checkpoints() {
    run(|driver| async move {
        for kind in ["handler_error", "protocol_fault"] {
            let rig = open(driver.clone(), vec![], |_| {}).await?;
            let mut task = spec("parent");
            task.input = json!({"mutate_before_result":true,(kind):true});
            rig.tasks.create(task).await?;
            let parked = status(&rig.tasks, "parent", TaskStatus::Parked).await?;
            assert_eq!(parked.checkpoint, json!({"newer_checkpoint":true}));
            assert_eq!(parked.phase, "newer-phase");
            assert!(parked.wake_at_ms.is_none());
            assert!(parked.wait_for.is_empty());
            assert_eq!(rig.control.effects("parent"), 1);
            rig.host.close().await?;
            rig.backend.close().await?;
        }
        Ok(())
    });
}

#[test]
fn escaped_error_diagnostics_fit_the_record_control_reserve() {
    run(|driver| async move {
        let rig = open(driver, vec![], |_| {}).await?;
        let mut task = spec("parent");
        task.input = json!({"data":"x".repeat(770_000),"control_error":true});
        let input = task.input.clone();
        let checkpoint = task.checkpoint.clone();
        rig.tasks.create(task).await?;
        let parked = status(&rig.tasks, "parent", TaskStatus::Parked).await?;
        assert_eq!(parked.input, input);
        assert_eq!(parked.checkpoint, checkpoint);
        let Some(BlockingReason::HandlerFailure { detail }) = parked.blocking_reason else {
            panic!("expected handler error");
        };
        assert!(task_wire_bytes(&detail)? <= 4096);
        assert_eq!(rig.control.effects("parent"), 1);
        rig.tasks.create(spec("small-after-diagnostic")).await?;
        assert!(matches!(
            rig.tasks
                .wait(TaskLookup {
                    id: "small-after-diagnostic".into()
                })
                .await?,
            TaskOutcome::Completed { .. }
        ));
        rig.host.close().await?;
        rig.backend.close().await
    });
}

#[test]
fn exhausted_attempt_metadata_faults_only_the_affected_task() {
    run(|driver| async move {
        let mut task = TaskRecord::new(spec("max-attempts"), None);
        task.attempts = u32::MAX;
        let checkpoint = task.checkpoint.clone();
        let rig = open(driver, vec![task], |_| {}).await?;
        let task = status(&rig.tasks, "max-attempts", TaskStatus::Terminal).await?;
        assert!(matches!(task.outcome, Some(TaskOutcome::Faulted { .. })));
        assert_eq!(task.checkpoint, checkpoint);
        assert_eq!(rig.control.effects("max-attempts"), 0);
        rig.tasks.create(spec("small-after-exhaustion")).await?;
        assert!(matches!(
            rig.tasks
                .wait(TaskLookup {
                    id: "small-after-exhaustion".into()
                })
                .await?,
            TaskOutcome::Completed { .. }
        ));
        rig.host.close().await?;
        rig.backend.close().await
    });
}

#[test]
fn repeated_family_contention_retains_the_actual_settled_handler_decision() {
    run(|driver| async move {
        let mut parent = spec("parent");
        parent.input = json!({"invalidate_after_effect":true});
        let mut records = vec![TaskRecord::new(parent, None)];
        for id in ["child-a", "child-b"] {
            let mut child = TaskRecord::new(spec(id), Some("parent".into()));
            child.status = TaskStatus::Terminal;
            child.outcome = Some(TaskOutcome::Completed { value: json!(null) });
            records.push(child);
        }
        let mut churn = TaskRecord::new(spec("churn"), None);
        churn.status = TaskStatus::Sleeping;
        churn.wake_at_ms = Some(9_000_000_000_000_000);
        records.push(churn);
        let rig = open(driver, records, |_| {}).await?;
        assert!(matches!(
            rig.tasks
                .wait(TaskLookup {
                    id: "parent".into()
                })
                .await?,
            TaskOutcome::Completed { .. }
        ));
        assert_eq!(rig.control.cursor_invalidations.get(), 0);
        assert_eq!(rig.control.effects("parent"), 1);
        rig.host.close().await?;
        rig.backend.close().await
    });
}

#[test]
fn child_inserted_after_cancellation_snapshot_is_reconciled_without_stopping() {
    run(|driver| async move {
        let mut parent = TaskRecord::new(spec("parent"), None);
        parent.status = TaskStatus::Sleeping;
        parent.wake_at_ms = Some(9_000_000_000_000_000);
        let mut leaf = TaskRecord::new(spec("leaf"), Some("parent".into()));
        leaf.status = TaskStatus::Sleeping;
        leaf.wake_at_ms = Some(9_000_000_000_000_000);
        let rig = open(driver, vec![parent, leaf], |control| {
            control.insert_child_during_cancel.set(true)
        })
        .await?;
        rig.tasks
            .cancel(TaskLookup {
                id: "parent".into(),
            })
            .await?;
        assert!(matches!(
            rig.tasks
                .wait(TaskLookup {
                    id: "parent".into()
                })
                .await?,
            TaskOutcome::Aborted { .. }
        ));
        assert!(!rig.control.insert_child_during_cancel.get());
        let child = rig
            .tasks
            .get(TaskLookup {
                id: "late-grandchild".into(),
            })
            .await?
            .unwrap();
        assert!(matches!(child.outcome, Some(TaskOutcome::Aborted { .. })));
        assert_eq!(rig.control.effects("late-grandchild"), 0);
        rig.tasks.create(spec("small-after-cancel-race")).await?;
        assert!(matches!(
            rig.tasks
                .wait(TaskLookup {
                    id: "small-after-cancel-race".into()
                })
                .await?,
            TaskOutcome::Completed { .. }
        ));
        rig.host.close().await?;
        rig.backend.close().await
    });
}

#[test]
fn a_child_inserted_during_complete_retains_the_settled_completion_decision() {
    run(|driver| async move {
        let rig = open(driver, vec![], |control| {
            control.insert_child_during_complete.set(true)
        })
        .await?;
        rig.tasks.create(spec("parent")).await?;
        let completing = status(&rig.tasks, "parent", TaskStatus::Completing).await?;
        assert!(
            matches!(completing.outcome, Some(TaskOutcome::Completed { .. })),
            "{completing:?}"
        );
        assert_eq!(rig.control.effects("parent"), 1);
        rig.tasks
            .cancel(TaskLookup {
                id: "late-owned-child".into(),
            })
            .await?;
        assert!(matches!(
            rig.tasks
                .wait(TaskLookup {
                    id: "parent".into()
                })
                .await?,
            TaskOutcome::Completed { .. }
        ));
        assert_eq!(rig.control.effects("parent"), 1);
        rig.host.close().await?;
        rig.backend.close().await
    });
}
