//! Real SQLite and managed operation races must not replay settled external phases.

use std::{cell::RefCell, future::Future, rc::Rc, time::Duration};

use futures::future::LocalBoxFuture;
use serde_json::json;
use swarmie_durable_tasks_fixture::concurrency::{
    CONTINUATION, StorageControl, handler_facet, storage_facet,
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
                outbox_legacy_owners: Default::default(),
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
                                provenance: None,
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

#[test]
fn a_root_phase_reads_its_family_only_for_admission_and_decision() {
    run(|driver| async move {
        let rig = open(driver, vec![], |_| {}).await?;
        // A completed resume includes startup discovery; count only the phase that follows.
        rig.tasks.resume().await?;
        rig.control.reset_counts();
        rig.tasks.create(spec("parent")).await?;
        assert!(matches!(
            rig.tasks
                .wait(TaskLookup {
                    id: "parent".into()
                })
                .await?,
            TaskOutcome::Completed { .. }
        ));
        assert_eq!(
            rig.control.commits.get(),
            3,
            "creation, reservation and decision are the only writes"
        );
        // Admission and decision each read one fenced family: an opening sequence probe and a
        // children page. The reconciled root record is the reservation base, and the engine
        // does not reconcile its own in-flight reservation. One bounded probe may also run.
        assert!(
            rig.control.scans.get() <= 5,
            "{} scans for one root phase",
            rig.control.scans.get()
        );
        assert_eq!(rig.control.effects("parent"), 1);
        rig.host.close().await?;
        rig.backend.close().await
    });
}

/// Records requested timer durations while the Host's actual driver performs scheduling.
struct TimerLog {
    inner: Rc<TokioDriver>,
    sleeps: RefCell<Vec<Duration>>,
}

impl core::Driver for TimerLog {
    fn spawn(&self, future: LocalBoxFuture<'static, ()>) {
        core::Driver::spawn(self.inner.as_ref(), future);
    }

    fn sleep(&self, duration: Duration) -> LocalBoxFuture<'static, ()> {
        self.sleeps.borrow_mut().push(duration);
        core::Driver::sleep(self.inner.as_ref(), duration)
    }

    fn now(&self) -> Duration {
        core::Driver::now(self.inner.as_ref())
    }
}

#[test]
fn scheduling_and_paged_reads_request_no_fixed_pacing_timers() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let backend = Host::with_driver("timer-backend", "application", driver.clone())?;
        backend
            .install(
                swarmie_task_storage_sqlite::facet(&swarmie_task_storage_sqlite::Configuration {
                    outbox_legacy_owners: Default::default(),
                    database: directory.path().join("tasks.sqlite"),
                    outbox: Default::default(),
                })?,
                core::LoadOptions::default(),
            )?
            .await?;
        let control = StorageControl::new(backend.get(TaskStorage, TaskStorageInput {}).await?);
        // One record per page makes every relation read follow continuation cursors.
        control.page_limit.set(1);
        let timers = Rc::new(TimerLog {
            inner: driver.clone(),
            sleeps: RefCell::new(Vec::new()),
        });
        let host = Host::with_driver("timer-scheduler", "application", driver.clone())?;
        host.install(
            storage_facet(control.clone())?,
            core::LoadOptions::default(),
        )?
        .await?;
        host.install(
            swarmie_durable_tasks_fixture::facet(1)?,
            core::LoadOptions::default(),
        )?
        .await?;
        host.install(
            swarmie_durable_tasks::facet(
                &swarmie_durable_tasks::Configuration {
                    poll_interval_ms: 2,
                    ..Default::default()
                },
                timers.clone(),
            )?,
            core::LoadOptions::default(),
        )?
        .await?;
        let tasks = host.get(DurableTasks, DurableTasksInput {}).await?;
        let mut parent = swarmie_durable_tasks_fixture::task("paged-parent");
        parent.phase = "spawn".into();
        tasks.create(parent).await?;
        assert!(matches!(
            tasks
                .wait(TaskLookup {
                    id: "paged-parent".into()
                })
                .await?,
            TaskOutcome::Completed { .. }
        ));
        control.page_limit.set(0);
        let graph = tasks.graph(TaskQuery::default()).await?;
        assert_eq!(graph.records.len(), 3);
        assert!(graph.records.iter().all(TaskRecord::is_terminal));
        let sleeps = timers.sleeps.borrow().clone();
        assert!(
            !sleeps.is_empty(),
            "the scheduler still arms its poll timer"
        );
        assert!(
            sleeps
                .iter()
                .all(|duration| *duration >= Duration::from_millis(2)),
            "phases and page reads yield without fixed pacing timers: {sleeps:?}"
        );
        host.close().await?;
        backend.close().await
    });
}

fn continued(id: &str) -> TaskSpec {
    let mut task = spec(id);
    task.input = json!({"continue":true});
    task
}

/// The confirmed writes of one task, as (status, phase, attempts, provenance).
fn writes(
    control: &StorageControl,
    id: &str,
) -> Vec<(TaskStatus, String, u32, Option<TaskTransitionProvenance>)> {
    control
        .committed
        .borrow()
        .iter()
        .filter(|write| write.record.id == id)
        .map(|write| {
            (
                write.record.status,
                write.record.phase.clone(),
                write.record.attempts,
                write.provenance,
            )
        })
        .collect()
}

#[test]
fn a_continuation_commits_its_decision_and_next_reservation_in_one_write() {
    run(|driver| async move {
        let rig = open(driver, vec![], |_| {}).await?;
        rig.tasks.create(continued("parent")).await?;
        assert_eq!(
            rig.tasks
                .wait(TaskLookup {
                    id: "parent".into()
                })
                .await?,
            TaskOutcome::Completed {
                value: json!({"completed_after_continuation":true})
            }
        );
        let handler = |checkpoint_committed| {
            Some(TaskTransitionProvenance {
                origin: TaskLifecycleOrigin::Handler,
                checkpoint_committed,
            })
        };
        assert_eq!(
            writes(&rig.control, "parent"),
            [
                (
                    TaskStatus::Pending,
                    "effect".into(),
                    0,
                    Some(TaskTransitionProvenance {
                        origin: TaskLifecycleOrigin::Creation,
                        checkpoint_committed: false
                    })
                ),
                (
                    TaskStatus::Running,
                    "effect".into(),
                    1,
                    Some(TaskTransitionProvenance {
                        origin: TaskLifecycleOrigin::Reservation,
                        checkpoint_committed: false
                    })
                ),
                (
                    TaskStatus::Running,
                    "complete".into(),
                    2,
                    Some(CONTINUATION)
                ),
                (TaskStatus::Terminal, "complete".into(), 2, handler(false)),
            ],
            "the effect decision and the next reservation share one revision"
        );
        let record = rig
            .tasks
            .get(TaskLookup {
                id: "parent".into(),
            })
            .await?
            .unwrap();
        assert_eq!(record.revision, 4);
        assert_eq!(record.restarts, 0);
        assert_eq!(record.checkpoint, json!({"effect_performed":true}));
        assert_eq!(rig.control.effects("parent"), 1);
        assert_eq!(rig.control.completions("parent"), 1);
        rig.host.close().await?;
        rig.backend.close().await
    });
}

#[test]
fn concurrent_cancellation_wins_over_a_settled_continuation() {
    run(|driver| async move {
        let rig = open(driver, vec![], |control| {
            control.pause_continuation_commit.set(true)
        })
        .await?;
        let parent = continued("parent");
        let checkpoint = parent.checkpoint.clone();
        rig.tasks.create(parent).await?;
        while !rig.control.continuation_commit_started.get() {
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
        let record = rig
            .tasks
            .get(TaskLookup {
                id: "parent".into(),
            })
            .await?
            .unwrap();
        assert_eq!(
            record.checkpoint, checkpoint,
            "the conflicting continuation never committed"
        );
        assert_eq!(record.phase, "effect");
        assert_eq!(rig.control.effects("parent"), 1);
        assert_eq!(rig.control.completions("parent"), 0);
        rig.host.close().await?;
        rig.backend.close().await
    });
}

#[test]
fn cancellation_after_a_continued_reservation_blocks_the_next_invocation() {
    run(|driver| async move {
        let rig = open(driver, vec![], |control| {
            control.cancel_after_continuation.set(true)
        })
        .await?;
        rig.tasks.create(continued("parent")).await?;
        assert!(matches!(
            rig.tasks
                .wait(TaskLookup {
                    id: "parent".into()
                })
                .await?,
            TaskOutcome::Aborted { .. }
        ));
        assert!(!rig.control.cancel_after_continuation.get());
        let record = rig
            .tasks
            .get(TaskLookup {
                id: "parent".into(),
            })
            .await?
            .unwrap();
        // The continuation committed the decision and the reservation; admission then saw the
        // later intent and settled the task without entering the continued phase.
        assert_eq!(record.checkpoint, json!({"effect_performed":true}));
        assert_eq!(record.phase, "complete");
        assert_eq!(record.attempts, 2);
        assert_eq!(rig.control.effects("parent"), 1);
        assert_eq!(rig.control.completions("parent"), 0);
        rig.host.close().await?;
        rig.backend.close().await
    });
}

#[test]
fn a_continuation_commits_pending_while_other_work_waits_for_a_full_slot() {
    run(|driver| async move {
        // Both roots exist before the single-slot scheduler starts, so the second waits while
        // the first runs and asks to continue.
        let records = vec![
            TaskRecord::new(continued("a-continued"), None),
            TaskRecord::new(spec("b-waiting"), None),
        ];
        let rig = open(driver, records, |_| {}).await?;
        for id in ["a-continued", "b-waiting"] {
            assert!(matches!(
                rig.tasks.wait(TaskLookup { id: id.into() }).await?,
                TaskOutcome::Completed { .. }
            ));
        }
        let handler = Some(TaskTransitionProvenance {
            origin: TaskLifecycleOrigin::Handler,
            checkpoint_committed: true,
        });
        let continued = writes(&rig.control, "a-continued");
        assert_eq!(
            continued[1],
            (TaskStatus::Pending, "complete".into(), 1, handler),
            "with every slot occupied and work waiting, the decision is an ordinary checkpoint"
        );
        assert!(continued.iter().all(|write| write.3 != Some(CONTINUATION)));
        let order: Vec<_> = rig
            .control
            .committed
            .borrow()
            .iter()
            .map(|write| {
                (
                    write.record.id.clone(),
                    write.record.status,
                    write.record.phase.clone(),
                )
            })
            .collect();
        let waiting_done = order
            .iter()
            .position(|write| write.0 == "b-waiting" && write.1 == TaskStatus::Terminal)
            .unwrap();
        let next_phase = order
            .iter()
            .position(|write| {
                write.0 == "a-continued" && write.1 == TaskStatus::Running && write.2 == "complete"
            })
            .unwrap();
        assert!(
            waiting_done < next_phase,
            "the waiting task runs before the next phase: {order:?}"
        );
        assert_eq!(rig.control.effects("a-continued"), 1);
        assert_eq!(rig.control.completions("a-continued"), 1);
        assert_eq!(rig.control.effects("b-waiting"), 1);
        rig.host.close().await?;
        rig.backend.close().await
    });
}
