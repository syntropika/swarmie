//! Kills actual processes after an acknowledged intention and external effect, then reopens SQLite.

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    rc::Rc,
    thread,
    time::{Duration, Instant},
};

use serde_json::json;
use swarmie_durable_tasks_fixture::{
    facet,
    hooks::{self, Observer},
    task,
};
use swarmie_task_contracts::*;
use tembra::{core, prelude::*};
use tembra_native::TokioDriver;

fn child(database: &Path, log: &Path, phase: &str) -> Child {
    let mut command = Command::new(env::current_exe().unwrap());
    command
        .args(["--exact", "child_process", "--nocapture"])
        .env("SWARMIE_TASK_FIXTURE_PHASE", phase)
        .env("SWARMIE_TASK_FIXTURE_DATABASE", database)
        .env("SWARMIE_TASK_FIXTURE_LOG", log)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if phase == "interrupt" {
        command.env("SWARMIE_TASK_FIXTURE_HANG_AFTER_EFFECT", "1");
    }
    command.spawn().unwrap()
}

fn effect_count(log: &Path) -> usize {
    fs::read_to_string(log)
        .map(|text| text.lines().count())
        .unwrap_or(0)
}

fn interrupt_after_effect(database: &Path, log: &Path, expected: usize) {
    let mut child = child(database, log, "interrupt");
    let started = Instant::now();
    while effect_count(log) < expected {
        if let Some(status) = child.try_wait().unwrap() {
            panic!("workflow process exited before effect: {status}");
        }
        if started.elapsed() > Duration::from_secs(15) {
            child.kill().unwrap();
            panic!("workflow did not reach external effect");
        }
        thread::sleep(Duration::from_millis(5));
    }
    child.kill().expect("terminate the real executing process");
    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
}

fn finish(database: &Path, log: &Path, phase: &str) {
    let output = child(database, log, phase).wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn killed_effect_repeats_with_identical_operation_key_and_recovered_checkpoint() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("tasks.sqlite");
    let log = directory.path().join("effects.log");
    interrupt_after_effect(&database, &log, 1);
    finish(&database, &log, "complete");
    let lines = fs::read_to_string(&log)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    assert_eq!(lines, ["process-task:publish", "process-task:publish"]);
}

#[test]
fn repeated_process_crashes_park_and_explicit_retry_preserves_checkpoint() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("tasks.sqlite");
    let log = directory.path().join("effects.log");
    for count in 1..=3 {
        interrupt_after_effect(&database, &log, count);
    }
    finish(&database, &log, "retry");
    assert_eq!(effect_count(&log), 4);
    assert!(
        fs::read_to_string(log)
            .unwrap()
            .lines()
            .all(|line| line == "process-task:publish")
    );
}

#[test]
fn killed_after_terminal_commit_does_not_replay_an_undelivered_notification() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("tasks.sqlite");
    let log = directory.path().join("effects.log");
    let committed = log.with_extension("committed");
    let mut child = child(&database, &log, "commit-gap");
    let started = Instant::now();
    while !committed.exists() {
        if let Some(status) = child.try_wait().unwrap() {
            let output = child.wait_with_output().unwrap();
            panic!(
                "workflow exited before terminal commit: {status}; stdout={} stderr={}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr),
            );
        }
        if started.elapsed() > Duration::from_secs(15) {
            child.kill().unwrap();
            panic!("workflow did not acknowledge terminal commit");
        }
        thread::sleep(Duration::from_millis(5));
    }
    child
        .kill()
        .expect("terminate after confirmed commit and before terminal observation");
    assert!(!child.wait_with_output().unwrap().status.success());
    finish(&database, &log, "reopen-terminal");
    assert_eq!(
        effect_count(&log),
        1,
        "reopening terminal work must not replay its handler"
    );
}

async fn drained(host: &Host) -> Result<()> {
    let lifecycle = host.get(TaskLifecycle, TaskLifecycleInput {}).await?;
    loop {
        let diagnostics = lifecycle.diagnostics().await?;
        if diagnostics.queued == 0 && !diagnostics.active {
            assert_eq!(diagnostics.failed, 0);
            assert_eq!(diagnostics.dropped, 0);
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
}

#[test]
fn child_process() -> Result<()> {
    let Some(phase) = env::var_os("SWARMIE_TASK_FIXTURE_PHASE") else {
        return Ok(());
    };
    let database = PathBuf::from(env::var_os("SWARMIE_TASK_FIXTURE_DATABASE").unwrap());
    let log = PathBuf::from(env::var_os("SWARMIE_TASK_FIXTURE_LOG").unwrap());
    let driver = TokioDriver::new();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(driver.run_until(async {
            tokio::time::timeout(Duration::from_secs(15), async {
                let host = Host::with_driver("process-durable", "application", driver.clone())?;
                host.install(
                    swarmie_task_storage_sqlite::facet(
                        &swarmie_task_storage_sqlite::Configuration { database },
                    )?,
                    core::LoadOptions::default(),
                )?
                .await?;
                host.install(facet(1)?, core::LoadOptions::default())?
                    .await?;
                let observer = Rc::new(Observer::default());
                observer.hold.set(phase == "commit-gap");
                // Startup recovery can commit immediately when the scheduler becomes ready.
                host.install(
                    hooks::facet("process-audit", 0, observer.clone())?,
                    core::LoadOptions::default(),
                )?
                .await?;
                host.install(
                    swarmie_durable_tasks::facet(
                        &swarmie_durable_tasks::Configuration {
                            poll_interval_ms: 2,
                            ..Default::default()
                        },
                        driver.clone(),
                    )?,
                    core::LoadOptions::default(),
                )?
                .await?;
                let tasks = host.get(DurableTasks, DurableTasksInput {}).await?;
                if tasks
                    .get(TaskLookup {
                        id: "process-task".into(),
                    })
                    .await?
                    .is_none()
                {
                    let mut spec = task("process-task");
                    spec.input = json!({"effect_log":log});
                    tasks.create(spec).await?;
                }
                if phase == "retry" {
                    loop {
                        let record = tasks
                            .get(TaskLookup {
                                id: "process-task".into(),
                            })
                            .await?
                            .unwrap();
                        if record.status == TaskStatus::Parked {
                            assert_eq!(record.restarts, 3);
                            assert_eq!(record.blocking_reason, Some(BlockingReason::RestartLimit));
                            assert_eq!(
                                record.checkpoint,
                                json!({"operation_id":"process-task:publish"})
                            );
                            tasks.retry(TaskLookup { id: record.id }).await?;
                            break;
                        }
                        tokio::time::sleep(Duration::from_millis(2)).await;
                    }
                }
                assert!(matches!(
                    tasks
                        .wait(TaskLookup {
                            id: "process-task".into()
                        })
                        .await?,
                    TaskOutcome::Completed { .. }
                ));
                let record = tasks
                    .get(TaskLookup {
                        id: "process-task".into(),
                    })
                    .await?
                    .unwrap();
                assert_eq!(record.phase, "finish");
                if phase == "commit-gap" {
                    assert_eq!(record.attempts, 3);
                    while !observer.started.get() {
                        tokio::time::sleep(Duration::from_millis(2)).await;
                    }
                    // Retained physical ownership blocks publication of later queued events,
                    // while task state and external work have already completed independently.
                    assert!(!observer.events.borrow().iter().any(|(_, event)| matches!(
                        event,
                        TaskLifecycleEvent::Committed {
                            current_status: TaskStatus::Terminal,
                            ..
                        }
                    )));
                    fs::write(
                        log.with_extension("committed"),
                        "terminal committed before notification",
                    )
                    .expect("acknowledge the persisted terminal state");
                    futures::future::pending::<()>().await;
                }
                for _ in 0..5 {
                    tasks.resume().await?;
                }
                drained(&host).await?;
                {
                    let events = observer.events.borrow();
                    if phase == "reopen-terminal" {
                        assert_eq!(record.attempts, 3);
                        assert!(
                            events.is_empty(),
                            "retained terminal state does not replay lifecycle observations"
                        );
                    } else {
                        assert!(record.attempts >= 4);
                        assert_eq!(
                            events
                                .iter()
                                .filter(|(_, event)| matches!(
                                    event,
                                    TaskLifecycleEvent::Committed {
                                        current_status: TaskStatus::Terminal,
                                        ..
                                    }
                                ))
                                .count(),
                            1,
                            "normal reconciliation must not duplicate terminal notification"
                        );
                        let recovered: Vec<_> = events
                            .iter()
                            .filter_map(|(_, event)| match event {
                                TaskLifecycleEvent::Committed {
                                    task,
                                    previous_status: Some(TaskStatus::Running),
                                    current_status,
                                    origin: TaskLifecycleOrigin::Recovery,
                                    blocking,
                                    ..
                                } => Some((task, current_status, blocking)),
                                _ => None,
                            })
                            .collect();
                        assert_eq!(
                            recovered.len(),
                            1,
                            "one confirmed recovery follows this real process death"
                        );
                        assert_eq!(recovered[0].0.phase, "effect");
                        assert_eq!(recovered[0].0.task_id, "process-task");
                        if phase == "retry" {
                            assert_eq!(*recovered[0].1, TaskStatus::Parked);
                            assert_eq!(*recovered[0].2, Some(TaskBlockingKind::RestartLimit));
                            assert_eq!(
                                events
                                    .iter()
                                    .filter(|(_, event)| matches!(
                                        event,
                                        TaskLifecycleEvent::Committed {
                                            previous_status: Some(TaskStatus::Parked),
                                            current_status: TaskStatus::Pending,
                                            origin: TaskLifecycleOrigin::Retry,
                                            ..
                                        }
                                    ))
                                    .count(),
                                1
                            );
                        } else {
                            assert_eq!(*recovered[0].1, TaskStatus::Pending);
                            assert_eq!(*recovered[0].2, None);
                        }
                        assert!(events.iter().any(|(_, event)| matches!(
                            event,
                            TaskLifecycleEvent::Committed {
                                checkpoint_committed: true,
                                ..
                            }
                        )));
                        assert!(events.iter().any(|(_, event)| matches!(
                            event,
                            TaskLifecycleEvent::PhaseSettled {
                                outcome: TaskPhaseOutcome::Returned,
                                ..
                            }
                        )));
                    }
                }
                host.close().await
            })
            .await
            .expect("subprocess durable lifecycle timed out")
        }))
}
