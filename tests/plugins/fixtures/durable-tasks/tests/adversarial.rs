use std::{future::Future, rc::Rc, time::Duration};

use swarmie_durable_tasks_fixture::{
    adversarial::{self, Control},
    concurrency::{self, StorageControl},
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
            tokio::time::timeout(Duration::from_secs(10), test(driver.clone()))
                .await
                .expect("adversarial task acceptance timed out")
                .unwrap();
        }));
}

async fn open(
    driver: Rc<TokioDriver>,
    version: u32,
) -> Result<(Host, DurableTasksRef, Rc<Control>, tempfile::TempDir)> {
    let directory = tempfile::tempdir().unwrap();
    let host = Host::with_driver("adversarial-tasks", "application", driver.clone())?;
    let control = Rc::new(Control::default());
    host.install(
        swarmie_task_storage_sqlite::facet(&swarmie_task_storage_sqlite::Configuration {
            database: directory.path().join("tasks.sqlite"),
            outbox: Default::default(),
        })?,
        core::LoadOptions::default(),
    )?
    .await?;
    host.install(
        adversarial::facet(control.clone(), version)?,
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
    Ok((host, tasks, control, directory))
}

fn spec(id: &str, phase: &str) -> TaskSpec {
    TaskSpec {
        id: id.into(),
        kind: "fixture.adversarial".into(),
        definition_version: 1,
        input: serde_json::Value::Null,
        checkpoint: serde_json::json!({"original":true}),
        phase: phase.into(),
    }
}

async fn status(tasks: &DurableTasksRef, id: &str, expected: TaskStatus) -> Result<TaskRecord> {
    loop {
        let record = tasks.get(TaskLookup { id: id.into() }).await?.unwrap();
        if record.status == expected {
            return Ok(record);
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
}

#[test]
fn logical_cancellation_waits_for_actual_physical_settlement() {
    run(|driver| async move {
        let (host, tasks, control, _directory) = open(driver, 1).await?;
        tasks.create(spec("gated", "gate")).await?;
        while !control.started.get() {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        let cancelled = tasks.cancel(TaskLookup { id: "gated".into() }).await?;
        assert!(cancelled.cancellation_requested);
        tokio::time::sleep(Duration::from_millis(20)).await;
        let before = tasks.get(TaskLookup { id: "gated".into() }).await?.unwrap();
        assert_eq!(before.status, TaskStatus::Running);
        assert_eq!(before.attempts, 1);
        assert_eq!(before.checkpoint, serde_json::json!({"original":true}));
        control.release();
        assert!(matches!(
            tasks.wait(TaskLookup { id: "gated".into() }).await?,
            TaskOutcome::Aborted { .. }
        ));
        assert_eq!(
            tasks
                .get(TaskLookup { id: "gated".into() })
                .await?
                .unwrap()
                .attempts,
            1
        );
        host.close().await
    });
}

#[test]
fn invalid_handler_transitions_fault_without_claiming_checkpoint_progress() {
    run(|driver| async move {
        let (host, tasks, _control, _directory) = open(driver, 1).await?;
        for phase in ["wrong-mode", "invalid-phase", "no-progress"] {
            tasks.create(spec(phase, phase)).await?;
            let parked = status(&tasks, phase, TaskStatus::Terminal).await?;
            assert!(matches!(parked.outcome, Some(TaskOutcome::Faulted { .. })));
            assert_eq!(parked.checkpoint, serde_json::json!({"original":true}));
            assert_eq!(parked.phase, phase);
        }
        host.close().await
    });
}

#[test]
fn an_unexpected_handler_exception_parks_for_explicit_retry() {
    run(|driver| async move {
        let (host, tasks, _control, _directory) = open(driver, 1).await?;
        tasks.create(spec("exception", "exception")).await?;
        let first = status(&tasks, "exception", TaskStatus::Parked).await?;
        assert!(matches!(
            first.blocking_reason,
            Some(BlockingReason::HandlerFailure { .. })
        ));
        tasks
            .retry(TaskLookup {
                id: "exception".into(),
            })
            .await?;
        let second = status(&tasks, "exception", TaskStatus::Parked).await?;
        assert_eq!(second.attempts, first.attempts + 1);
        assert_eq!(second.checkpoint, first.checkpoint);
        host.close().await
    });
}

#[test]
fn migration_exception_blocks_once_and_preserves_checkpoint_version() {
    run(|driver| async move {
        let (host, tasks, _control, _directory) = open(driver, 2).await?;
        tasks.create(spec("migration", "wrong-mode")).await?;
        let blocked = status(&tasks, "migration", TaskStatus::Blocked).await?;
        assert!(matches!(
            blocked.blocking_reason,
            Some(BlockingReason::MigrationRejected {
                target_version: 2,
                ..
            })
        ));
        assert_eq!(blocked.definition_version, 1);
        assert_eq!(blocked.checkpoint, serde_json::json!({"original":true}));
        tokio::time::sleep(Duration::from_millis(20)).await;
        let unchanged = tasks
            .get(TaskLookup {
                id: "migration".into(),
            })
            .await?
            .unwrap();
        assert_eq!(unchanged.revision, blocked.revision);
        assert_eq!(unchanged.attempts, 1);
        host.close().await
    });
}

#[test]
fn cancellation_during_completing_preserves_the_decided_outcome() {
    run(|driver| async move {
        let (host, tasks, control, _directory) = open(driver, 1).await?;
        tasks.create(spec("parent", "spawn-failure")).await?;
        let completing = status(&tasks, "parent", TaskStatus::Completing).await?;
        assert!(matches!(
            completing.outcome,
            Some(TaskOutcome::Failed { .. })
        ));
        tasks
            .cancel(TaskLookup {
                id: "parent".into(),
            })
            .await?;
        tokio::time::sleep(Duration::from_millis(10)).await;
        let after = tasks
            .get(TaskLookup {
                id: "parent".into(),
            })
            .await?
            .unwrap();
        assert_eq!(after.outcome, completing.outcome);
        assert_eq!(after.status, TaskStatus::Completing);
        control.release();
        assert_eq!(
            tasks
                .wait(TaskLookup {
                    id: "parent".into()
                })
                .await?,
            completing.outcome.unwrap()
        );
        host.close().await
    });
}

#[test]
fn graceful_close_joins_physical_work_and_retries_without_counting_a_crash() {
    run(|driver| async move {
        let (host, tasks, control, directory) = open(driver.clone(), 1).await?;
        tasks.create(spec("graceful", "gate")).await?;
        while !control.started.get() {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        let close = host.close();
        tokio::pin!(close);
        tokio::select! {
            result = &mut close => panic!("Host closed before physical settlement: {result:?}"),
            _ = tokio::time::sleep(Duration::from_millis(20)) => {},
        }
        control.release();
        close.await?;

        let reopened = Host::with_driver("adversarial-reopen", "application", driver.clone())?;
        reopened
            .install(
                swarmie_task_storage_sqlite::facet(&swarmie_task_storage_sqlite::Configuration {
                    database: directory.path().join("tasks.sqlite"),
                    outbox: Default::default(),
                })?,
                core::LoadOptions::default(),
            )?
            .await?;
        let storage = reopened.get(TaskStorage, TaskStorageInput {}).await?;
        let preserved = storage
            .get(TaskLookup {
                id: "graceful".into(),
            })
            .await?
            .unwrap();
        assert_eq!(preserved.status, TaskStatus::Pending);
        assert_eq!(preserved.restarts, 0);
        assert_eq!(preserved.checkpoint, serde_json::json!({"original":true}));
        reopened
            .install(
                adversarial::facet(control, 1)?,
                core::LoadOptions::default(),
            )?
            .await?;
        reopened
            .install(
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
        let tasks = reopened.get(DurableTasks, DurableTasksInput {}).await?;
        assert!(matches!(
            tasks
                .wait(TaskLookup {
                    id: "graceful".into()
                })
                .await?,
            TaskOutcome::Completed { .. }
        ));
        let recovered = tasks
            .get(TaskLookup {
                id: "graceful".into(),
            })
            .await?
            .unwrap();
        assert_eq!(recovered.restarts, 0);
        assert_eq!(recovered.attempts, 2);
        reopened.close().await
    });
}

#[test]
fn an_older_registered_definition_cannot_downgrade_the_checkpoint() {
    run(|driver| async move {
        let (host, tasks, _control, _directory) = open(driver, 1).await?;
        let mut newer = spec("newer-checkpoint", "gate");
        newer.definition_version = 2;
        tasks.create(newer).await?;
        let blocked = status(&tasks, "newer-checkpoint", TaskStatus::Blocked).await?;
        assert!(matches!(
            blocked.blocking_reason,
            Some(BlockingReason::IncompatibleDefinition {
                target_version: 1,
                ..
            })
        ));
        assert_eq!(blocked.definition_version, 2);
        assert_eq!(blocked.attempts, 0);
        host.close().await
    });
}

#[test]
fn cancellation_of_a_large_tree_blocks_all_descendant_handler_admission() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let backend_host = Host::with_driver("cancel-tree-storage", "application", driver.clone())?;
        backend_host
            .install(
                swarmie_task_storage_sqlite::facet(&swarmie_task_storage_sqlite::Configuration {
                    database: directory.path().join("tasks.sqlite"),
                    outbox: Default::default(),
                })?,
                core::LoadOptions::default(),
            )?
            .await?;
        let storage = backend_host.get(TaskStorage, TaskStorageInput {}).await?;
        storage
            .claim_owner(StorageOwner {
                id: "fixture-writer".into(),
            })
            .await?;
        let mut root = TaskRecord::new(spec("tree", "gate"), None);
        let mut writes = vec![TaskWrite {
            provenance: None,
            expected_revision: None,
            record: root.clone(),
        }];
        for index in 0..255 {
            writes.push(TaskWrite {
                provenance: None,
                expected_revision: None,
                record: TaskRecord::new(
                    spec(&format!("child-{index:03}"), "gate"),
                    Some("tree".into()),
                ),
            });
        }
        storage
            .commit(TaskTransaction {
                owner: "fixture-writer".into(),
                writes,
            })
            .await?;
        storage
            .commit(TaskTransaction {
                owner: "fixture-writer".into(),
                writes: (0..4)
                    .map(|index| TaskWrite {
                        provenance: None,
                        expected_revision: None,
                        record: TaskRecord::new(
                            spec(&format!("grandchild-{index}"), "gate"),
                            Some("child-000".into()),
                        ),
                    })
                    .collect(),
            })
            .await?;
        root.cancellation_requested = true;
        root.revision = 2;
        storage
            .commit(TaskTransaction {
                owner: "fixture-writer".into(),
                writes: vec![TaskWrite {
                    provenance: None,
                    expected_revision: Some(1),
                    record: root,
                }],
            })
            .await?;
        storage
            .release_owner(StorageOwner {
                id: "fixture-writer".into(),
            })
            .await?;
        let host = Host::with_driver("cancel-tree", "application", driver.clone())?;
        let calls = StorageControl::new(storage);
        host.install(
            concurrency::storage_facet(calls.clone())?,
            core::LoadOptions::default(),
        )?
        .await?;
        let control = Rc::new(Control::default());
        host.install(
            adversarial::facet(control.clone(), 1)?,
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
        assert!(matches!(
            tasks.wait(TaskLookup { id: "tree".into() }).await?,
            TaskOutcome::Aborted { .. }
        ));
        let scan_count = calls.scans.get();
        assert!(
            calls.commits.get() <= 16,
            "bounded subtree cancellation must commit batches rather than individual leaves: {} commits",
            calls.commits.get()
        );
        assert!(
            scan_count <= 350,
            "bounded subtree sweeps must batch cancellation instead of reading every leaf family: {scan_count} scans"
        );
        let first = tasks
            .graph(TaskQuery {
                limit: 256,
                ..Default::default()
            })
            .await?;
        assert_eq!(first.records.len(), 256);
        assert!(first.records.iter().all(TaskRecord::is_terminal));
        let second = tasks
            .graph(TaskQuery {
                limit: 256,
                cursor: first.next,
                ..Default::default()
            })
            .await?;
        assert_eq!(second.records.len(), 4);
        assert!(second.records.iter().all(TaskRecord::is_terminal));
        assert!(!control.started.get());
        host.close().await?;
        backend_host.close().await
    });
}

#[test]
fn a_fatal_pump_error_is_visible_while_physical_work_is_still_draining() {
    run(|driver| async move {
        let (host, tasks, control, directory) = open(driver, 1).await?;
        tasks.create(spec("held", "gate")).await?;
        while !control.started.get() {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        let raw = rusqlite::Connection::open(directory.path().join("tasks.sqlite")).unwrap();
        raw.execute_batch("CREATE TRIGGER fail_terminal BEFORE UPDATE ON task_records WHEN OLD.id = 'fatal' AND NEW.status = 'terminal' BEGIN SELECT RAISE(FAIL, 'injected task commit failure'); END;").unwrap();
        tasks.create(spec("fatal", "complete")).await?;
        loop {
            match tasks.resume().await {
                Err(error) if error.reason == "durable_tasks_stopped" => break,
                Err(error) => return Err(error),
                Ok(_) => tokio::time::sleep(Duration::from_millis(2)).await,
            }
        }
        let rejected = tasks.create(spec("refused", "complete")).await.unwrap_err();
        assert_eq!(rejected.reason, "durable_tasks_stopped");
        let storage = host.get(TaskStorage, TaskStorageInput {}).await?;
        assert!(
            storage
                .get(TaskLookup {
                    id: "refused".into()
                })
                .await?
                .is_none()
        );
        assert_eq!(
            storage
                .get(TaskLookup { id: "held".into() })
                .await?
                .unwrap()
                .status,
            TaskStatus::Running
        );
        assert_eq!(
            storage
                .claim_owner(StorageOwner {
                    id: "competitor".into()
                })
                .await
                .unwrap_err()
                .reason,
            "task_storage_owned"
        );
        control.release();
        host.close().await
    });
}
