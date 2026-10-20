//! Kills actual processes after an acknowledged intention and external effect, then reopens SQLite.

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use serde_json::json;
use swarmie_durable_tasks_fixture::{facet, task};
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
                assert!(record.attempts >= 4);
                assert_eq!(record.phase, "finish");
                host.close().await
            })
            .await
            .expect("subprocess durable lifecycle timed out")
        }))
}
